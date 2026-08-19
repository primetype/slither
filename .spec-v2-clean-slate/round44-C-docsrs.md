# Round 44 — Agent C: docs.rs Navigation Audit

HEAD verified: `721167a` (Round 43: the SECV5 pins land — the one-shot
keepalive, the spent-packet separator, the 272 pin). Matches expected.

Scope: audit the rendered API-reference experience (docs.rs) as a
navigation problem — crate root to "message sent between two endpoints,"
by clicking. Doc comments (`//!`, `///`) in `src/**.rs` ARE the rendered
docs.

**One structural finding drives most of the rest, so it is stated up
front:** `src/shell/mod.rs` declares six of its seven submodules private
(`mod connection; mod driver; mod endpoint; mod shared; mod staged; mod
stream;` — only `pub mod wire;` is public), while re-exporting the public
*types* out of them (`pub use self::staged::{Claimed, Intro, Proven};`
etc.). rustdoc renders a doc page only for a `pub mod`. A struct's own
`///` doc travels with it to its re-exported path (e.g. `shell::Intro`
renders fine) — but each of those six files' **module-level `//!` doc
comment does not**, because there is no public module for it to attach
to. Concretely, this deletes from the rendered surface, in full:

- `staged.rs`'s ladder diagram with DH counts and verb names (the fullest
  version of Intro→Claimed→Proven→Connection in the crate)
- `endpoint.rs`'s "An endpoint is one socket's worth of protocol state…"
  orientation paragraph
- `connection.rs`'s overview of what `Connection` covers
- `driver.rs`'s description of the single `!Send` actor
- `stream.rs`'s "Where the work happens" explanation of the poll_* seam
- `shared.rs`'s description of the command channel / waker map

None of this is a lint failure (`missing_docs` only requires *an* item to
have `///`/`//!` somewhere, and every `pub` item here does) — it is prose
that was clearly written to orient a reader and is invisible to every
reader who arrives by clicking, because there is no click that reaches
it. It is reachable only via docs.rs's "source" view, landing on raw
syntax-highlighted source rather than a documentation page — a
fundamentally different (and much less likely) reading path. This is
flagged once here and referenced below rather than repeated at each
affected task.

## 1. Golden-path click-trace

Click counts assume the reader starts on the crate root page
(`slither` — `src/lib.rs`'s `//!`) and follows hyperlinks only.

**(a) Running the `!Send` stack (`block_on` / `LocalSet`)**
Answered conceptually at **0 clicks**: the crate root's own doc explains
the `!Send` actor / `LocalSet` requirement in its "Shape" section, and
`pub use compat::block_on;` is re-exported at the root with a doc pointing
to the full example. **1 click** (`block_on`) reaches
`src/compat/rt.rs`'s doc: a `no_run` fenced example building an endpoint,
dialling, opening a bi stream, writing, finishing and awaiting the ack —
the single best "whole shape in one call" example in the crate. Compiles
under `cargo test --doc` (confirmed below). Caveat: it is **one side
only** (the dialling side) — no peer, no accept, no message received.

**(b) Generate/load a static keypair; get your own public key for a peer**
Root → `identity` module (**1 click**, via the manual "Modules" prose
list; also present in rustdoc's auto-generated alphabetical table) →
`SoftwareIdentity` struct (**2 clicks**). The struct doc carries a
`no_run` fenced example (`src/identity.rs:189-201`) for seeding a
`ChaCha20Rng` from OS entropy — but the actual key-generation call is a
**comment**, not compiled code:
```rust
let rng = ChaCha20Rng::from_seed(seed);
// …then `SoftwareIdentity::<MySuite>::generate(rng)`.
```
`SoftwareIdentity::generate`'s own doc (**3 clicks**) has **no fence at
all** — only prose. `Identity::public_static` (the method that hands back
the bytes a peer needs out-of-band) also has **no fence anywhere in the
crate**: `grep -rn "public_static" src` turns up dozens of call sites, but
every one is inside `#[cfg(test)]` test code (`src/core/**/tests.rs`,
`src/testutil/mod.rs`), none in a doc comment. A reader who wants "print
my public key to send to a peer" has no compiled example to copy from
anywhere in the rendered docs. **Dead end.**

**(c) Building an `Endpoint` (`EndpointBuilder`)**
Root → `EndpointBuilder` (**1 click**, re-exported at root). The struct's
own doc carries a `no_run` fenced example (`src/shell/endpoint.rs:401-419`)
showing `.identity(identity).wire(wire).build()` inside a
`LocalSet::run_until`. This covers the two *required* builder methods but
not `.config(...)` or `.rng_seed(...)` (both optional, so this is a minor
gap, not a dead end). No page shows all four chained.

**(d) `connect()` + the `Connecting` future**
Root (**0 clicks**): the crate root's obligation #1 ("Reconnecting is
`close()` then dial") already contains a `no_run` compiled example
calling `endpoint.connect(addr, peer)?.await` correctly — but it is
framed entirely around the reconnect corner case, not a first connection.
Root → `Endpoint` (**1 click**) reaches `connect()`'s own doc: extensive
prose (cancel-safety, `AlreadyConnected` semantics, the "do not connect
ahead of need" warning) and one `text`-fenced (non-compiled) diagram of
the cancel-then-redial idiom — **no compiled example of a plain first
connect**. Root → `Connecting` (**1 click**) reaches the future type's own
doc: prose about cancellation ordering only, **zero code fences of any
kind**.

**(e) The staged accept ladder Intro→Claimed→Proven**
Root → `shell` (**1 click**): the module's own `//!` carries a compact
two-line `text` (non-compiled) diagram covering both the dial and accept
ladders together. The far more detailed version — DH counts and verb
names, `Intro (0 DH) → read_identity() +1 DH → Claimed (1 DH) →
authenticate() +1 DH → Proven (2 DH) → accept() +2 DH → Connection`, in
`src/shell/staged.rs`'s module doc — is **unreachable by clicking at all**
per the structural finding above (`staged` is a private `mod`). Root →
`Intro` / `Claimed` / `Proven` (**1 click** each, re-exported at root)
reach `read_identity()`, `authenticate()`, `accept()` respectively: each
has substantial prose but **no fenced example anywhere in `staged.rs`**
except the one unreachable module-level diagram. **No single page shows
the whole ladder as runnable code** — confirmed by exhaustive grep, this
does not exist anywhere in `src/`.

**(f) `send_message` / `recv_message`**
Root → `Connection` (**1 click**, re-exported at root; methods are on the
same struct page, no further click). `grep -n '```' src/shell/connection.rs`
returns exactly one fence in the whole file (on `notified()`, unrelated).
**Neither `send_message` nor `recv_message` has a code example of any
kind** — not even a `text` diagram. The one place the full idiom
(`send_message` → `acked` → `close`, on one side, `recv_message` on the
other) is actually written and exercised is a `#[tokio::test]` inside
`#[cfg(test)] mod tests` in `src/shell/mod.rs` (~line 1369-1404) — which,
being `#[cfg(test)]`, **never compiles into the crate's docs and never
appears on docs.rs at all**. This is the exact destination the audit
brief names ("message sent between two endpoints"), and it is the
**single biggest gap found**: 1 click to arrive, 0 examples once there.

**(g) tokio features and companion crates a consumer must declare**
`hiss`: documented and reachable. The crate root's `pub use hiss;` doc
(**0 clicks** — renders inline at root) carries a `toml` fence
(`slither = "0.2"` / `hiss = "0.3"`); the `channel!` macro doc
(root → `packet` → `channel`, **2 clicks**) repeats the same `toml` fence
verbatim. `tokio`: **undocumented in the rendered surface**. `tokio` is a
**hard, non-optional** dependency of slither with `features = ["rt",
"net", "time", "sync", "macros"]` baked directly into slither's own
`Cargo.toml` (line 129) — meaning a consumer does not need to enable
anything for the five features slither itself needs — but this fact
exists only as a `Cargo.toml` comment, which docs.rs never renders.
Nothing in `src/**.rs` doc comments states it. A reader who wonders "what
tokio features does my own `Cargo.toml` need" gets no answer anywhere in
the rendered reference; the closest hint is `compat::rt::block_on`'s
example, which never has to answer the question because it builds its
own runtime internally. Crate feature flags (`test-util`, `sink`,
`codec`, `tower`) **are** well documented, in the root's `# Features`
table (**0 clicks**).

## 2. Doctest inventory

`grep -rn '```' src --include='*.rs'` → 78 matches = 39 fences total.
By kind:

| Kind | Count | Files |
|---|---|---|
| plain / rust (compiled **and run**) | 4 | `config.rs:270`, `compat/io.rs:105`, `packet/suite.rs:177`, `testutil/mod.rs:267` |
| `no_run` (compiled only) | 7 | `identity.rs:189`, `lib.rs:61`, `compat/rt.rs:42`, `shell/connection.rs:266`, `shell/endpoint.rs:401`, `testutil/capture.rs:18`, `testutil/mod.rs:1195` |
| `text` (nothing) | 26 | `lib.rs:24`; `compat/mod.rs:9`; `compat/stream.rs` ×14; `compat/tower.rs` ×2; `core/connection/send.rs:3`; `shell/staged.rs:8` (unreachable, see above); `shell/stream.rs:409`; `shell/driver.rs:441`; `shell/mod.rs:7`; `shell/endpoint.rs:192`; `packet/mac.rs:3`; `packet/payload.rs:3` |
| `toml` (not rust — skipped entirely, not even "ignore"d) | 2 | `lib.rs:325`, `packet/suite.rs:198` |

Ran the two permitted commands:

```
$ git -C /Users/nicolasdiprima/work/primetype/slither log -1 --oneline
721167a Round 43: the SECV5 pins land — the one-shot keepalive, the spent-packet separator, the 272 pin

$ cargo test --doc --all-features 2>&1 | tail -20
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.02s
   Doc-tests slither

running 14 tests
test src/testutil/capture.rs - testutil::capture (line 18) - compile ... ok
test src/identity.rs - identity::SoftwareIdentity (line 189) - compile ... ok
test src/shell/endpoint.rs - shell::endpoint::EndpointBuilder (line 401) - compile ... ok
test src/lib.rs - (line 61) - compile ... ok
test src/compat/rt.rs - compat::rt::block_on (line 42) - compile ... ok
test src/testutil/mod.rs - testutil::Pair (line 1195) - compile ... ok
test src/shell/connection.rs - shell::connection::Connection<S>::notified (line 266) - compile ... ok
test src/packet/suite.rs - packet::suite::IK (line 275) ... ok
test src/packet/suite.rs - packet::suite::IK (line 311) ... ok
test src/packet/suite.rs - packet::suite::IK (line 243) ... ok
test src/packet/suite.rs - packet::suite::channel (line 177) ... ok
test src/compat/io.rs - compat::io::io::Error (line 105) ... ok
test src/config.rs - config::Config::with_flow_windows (line 270) ... ok
test src/testutil/mod.rs - testutil::FlakyPolicy::fail_sends (line 267) ... ok

test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.04s

all doctests ran in 4.36s; merged doctests compilation took 0.32s
```

14 doctests total: 7 `compile`-only (the `no_run` set above) and 7 run.
The run count (7) is one more than the 4 plain-rust fences my grep found
in slither's own source — the extra 3 (`packet::suite::IK` at lines 243,
275, 311) are **not authored by slither**: they are doc comments written
inside `hiss::noise!`'s macro expansion, surfaced through slither's
`crate::channel!` invocation at `src/packet/suite.rs:472`, and rustdoc
attributes their source location back to the `macro_rules!` body (lines
225-248) that defines `channel!`. On docs.rs these render as examples on
`packet::suite::IK`'s page — low-level Noise message-writing internals
(`write_message_1`, `read_message_1`, etc.), not slither's connect/accept
API. A reader who clicks into `IK` expecting "the suite my app declares"
lands on hiss-authored cryptography internals instead — a minor
dead-end/distraction, not counted among slither's own doctests.

**End-to-end coverage: none.** The closest candidate, `testutil::Pair`'s
`no_run` example (`testutil/mod.rs:1195`), establishes two endpoints on
one in-memory network and closes the connection — but sends no
application data. **Zero doctests in the whole rendered surface exercise
"a message sent between two endpoints"** — the exact scenario this audit
targets, and the only place that flow is written down at all is the
`#[cfg(test)]` unit test noted in 1(f), invisible to docs.rs.

## 3. Module-doc quality sweep

Every one of the seven audited modules opens its very first sentence with
a spec-section or ruling citation — a reviewer asking "which part of
`SPEC.md` does this satisfy," not a user asking "what do I do with this."

| File | First line (quoted verbatim) | Orientation |
|---|---|---|
| `src/shell/mod.rs` | *"The I/O shell — `SPEC.md` §16."* | Reviewer |
| `src/identity.rs` | *"The identity seam — the `I` in §16.4's `core::Endpoint<I: Identity>`."* | Reviewer |
| `src/config.rs` | *"Endpoint configuration, and §16.5's injected wall clock."* | Mixed — user noun phrase, immediately anchored to a section |
| `src/compat/mod.rs` | *"§16.11's composability surface: slither's verbs, in the shapes the async ecosystem consumes."* | Reviewer |
| `src/packet/mod.rs` | *"§2–§5 — the wire: where bytes acquire meaning."* | Reviewer |
| `src/constants.rs` | *"Every named constant `SPEC.md` fixes, and nothing else."* | Reviewer (module is legitimately reference-only, so this is less costly) |
| `src/error.rs` | *"The error taxonomy — `SPEC.md` §18.1, plus the one type it excludes."* | Reviewer |

8/8 of the crate's `pub mod` module docs (this list plus `shell::wire`,
*"The datagram substrate seam — `SPEC.md` §16.3."*) open the same way —
100%. The pattern repeats at the golden-path item level: `EndpointBuilder::identity`
opens *"The static-key seam (§2.4). Required."*; `Endpoint::connect` opens
*"Dial `remote_static` at `remote` (§5.5)."*; `Endpoint::accept` opens
*"Wait for the next introduction (§6.2, §6.3)."*; `send_message` opens
*"Send one reliable-unordered message (§9.8)."* — nearly every public
verb's first sentence carries a `(§X.Y)` suffix as reflex.

Quantified across the whole crate (`grep -rhE '^\s*(///|//!)' src
--include='*.rs'`): 21,929 doc-comment lines total, 3,521 (16%) contain a
`§` section reference, 1,159 (5.3%) mention a ruling. Per-file, for the
seven audited files specifically:

| File | doc lines | lines with `§` | % |
|---|---|---|---|
| `shell/mod.rs` | 338 | 30 | 9% |
| `identity.rs` | 181 | 11 | 6% |
| `config.rs` | 204 | 43 | 21% |
| `compat/mod.rs` | 83 | 9 | 11% |
| `packet/mod.rs` | 103 | 22 | 21% |
| `constants.rs` | 322 | 116 | 36% |
| `error.rs` | 343 | 46 | 13% |

The raw body-text percentage (6-36%) is not alarming on its own — much of
it is legitimately-cited implementation rationale deep in a page. What is
striking is the **positional** concentration: the very first sentence a
reader sees on every module page, and on nearly every golden-path verb,
is reviewer-oriented, regardless of how good the prose that follows it
is (and in `shell/endpoint.rs`, `staged.rs`, `identity.rs` the prose that
follows is often excellent — see `identity.rs`'s explanation of why
`Identity::open` is a factory, or `EndpointBuilder::rng_seed`'s security
rationale). The defect is not absent user-facing writing; it is that the
user-facing writing never leads.

## 4. Dead-end check

docs.rs auto-generates a "Modules" summary table on the crate root,
**alphabetical by module name**, from every `pub mod`. With
`all-features = true` set in `Cargo.toml`'s `[package.metadata.docs.rs]`
(confirmed — comment explains this is deliberate so `testutil` renders),
that table is:

**compat, config, constants, error, identity, packet, shell, testutil**

A newcomer needs, in priority order: **identity** (get/load keys) →
**shell** (Endpoint, connect/accept, Connection, send_message) →
**config** (optional, defaults work) → **error** (what can go wrong) →
**packet** (only if declaring your own suite) → **constants** (reference)
→ **compat** (only once already using streams, for `AsyncRead`/`tower`)
→ **testutil** (tests only).

The rendered table puts `compat` — the module nobody needs before they
already have a working `Connection` and want it as an `AsyncRead` — in
the very first slot, ahead of both `identity` and `shell`. `identity` sits
5th of 8; `shell`, the module holding `Endpoint`, `Connection`,
`connect()`, `accept()` and `send_message`, sits 7th of 8, second-to-last,
just ahead of `testutil`. Alphabetical ordering is a rustdoc default a
crate cannot override with more `pub mod`s — only prose above it can
compensate — and the crate root's own hand-written prose section headed
`# Modules` (lib.rs:181-206) also does not lead with identity or shell:
its order is constants, error, packet, identity, config, `core`
(unlinked — `pub(crate)`), shell, compat, testutil. Identity is 4th of 8
listed there and shell 6th, better than the auto table but still not
leading with what a newcomer needs first. Net effect: **two different
module orderings on one page (the hand-written prose list and the
rustdoc-generated table directly below it), and neither puts identity or
shell first.**

## The five highest-leverage reference-doc fixes

1. **Make the shell's staged/endpoint/connection/stream module docs
   public and clickable.** `src/shell/mod.rs:52-57` declares
   `connection`, `driver`, `endpoint`, `shared`, `staged`, `stream` as
   private `mod`, so their `//!` headers — including `staged.rs`'s DH-
   annotated ladder diagram (the fullest rendering of Intro→Claimed→
   Proven→Connection in the crate) and `endpoint.rs`'s orientation
   paragraph — never appear on docs.rs at all. Change the ones that carry
   reader-facing prose (`staged`, `endpoint`, `connection`, `stream` at
   minimum; `driver`/`shared` are legitimately internal) to `pub mod`, or
   fold their opening paragraphs into `shell/mod.rs`'s own `//!` (already
   public) so the content survives even if the module stays private.

2. **Put a runnable two-endpoint message example on
   `Connection::send_message` / `Connection::recv_message`**
   (`src/shell/connection.rs:700`, `:745`). Both currently have zero code
   fences — the literal target this audit was asked to reach. The
   existing `send_message` → `acked` → `close` / `recv_message` idiom
   already exists, fully exercised, as a `#[cfg(test)]` unit test in
   `src/shell/mod.rs` (~line 1369); lift it (behind `test-util`, `no_run`
   or `ignore` since it needs the paused clock) into a doc example on
   both methods.

3. **Give `SoftwareIdentity::generate` and `Identity::public_static` real
   compiled examples** (`src/identity.rs`, the `generate` method has no
   fence at all; `public_static` likewise). The existing RNG-seed example
   on the struct (`identity.rs:189`) stops one line short of the actual
   call — `SoftwareIdentity::<MySuite>::generate(rng)` is a `//` comment,
   not compiled code — and no fence anywhere calls `.public_static()` or
   shows `.as_ref()` to get the bytes for out-of-band exchange with a
   peer. This is explicitly named in the audit brief and today it is a
   dead end.

4. **Add a "Start here" quick-path at the very top of `src/lib.rs`'s
   crate doc**, before the "Shape" section, linking `identity` →
   `EndpointBuilder` → `connect`/`accept` → `send_message`/`recv_message`
   in that order. The rustdoc-generated Modules table is alphabetical and
   cannot be reordered (`compat` before `identity` before `shell`); the
   only lever available is prose above it, and the crate doc's own
   hand-written `# Modules` list (lib.rs:181) doesn't lead with them
   either.

5. **Add a plain first-connect example to `Endpoint::connect` or
   `Connecting`** (`src/shell/endpoint.rs:218`, `:285`). Today the only
   compiled example touching `connect()` is the crate root's "how to
   reconnect" obligation, framed around releasing a stale static — not
   "how do I connect to a peer the first time." `connect()`'s own doc has
   only a `text` (non-compiled) diagram, and `Connecting`'s struct page
   has zero fences of any kind.

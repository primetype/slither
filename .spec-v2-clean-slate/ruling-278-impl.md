# Ruling 278 implementation notes

## Base
- Expected: 7896a67 ("Ruling 278: the prelude replaces the flat root")
- Actual: `git log -1 --oneline` -> 7896a67 Ruling 278: the prelude replaces the flat root
- `git status --porcelain` -> (empty; clean tree)
- VERIFIED.

## Manifest (src/prelude.rs)

Exactly the ruling's list, nothing more:

```rust
pub use crate::compat::block_on;
pub use crate::config::Config;
pub use crate::identity::{Identity, SoftwareIdentity};
pub use crate::shell::wire::Wire;
pub use crate::shell::{
    BiStream, Claimed, Connecting, Connection, Endpoint, EndpointBuilder, Intro, Notification,
    Proven, RecvStream, SendStream,
};
pub use hiss::noise::{Blake2b, ChaChaPoly, P256};
```

Module doc covers: what it is, `use slither::prelude::*;`, the
trait-in-scope rationale, the one blessed spelling for everything else, the
three hiss types (one line, linking `[`slither::hiss`](crate::hiss)` for the
manifest requirement that stays), and the growth policy stamped
**[RATIFIED 2026/08/19 — ruling 278] This module changes only by ruling.**

## Files touched

**New**
- `src/prelude.rs` — the golden-path prelude.

**Root surface**
- `src/lib.rs` — `pub mod prelude;` added; the six root re-export blocks
  removed (`block_on` + its 12-line doc, `config::{Config, SystemClock,
  WallClock}`, the ten-error block, `identity::{…}`, `packet::{Channel,
  Handshake}`, `shell::{…}`); `SessionId` / `Dir,StreamId,Timestamp` /
  `pub use hiss` blocks kept (see the one forced exception below); crate-doc
  quickstart now opens on `use slither::prelude::*;`; `Modules` list gains a
  `prelude` bullet and the `error` bullet rewritten module-only; every bare
  intra-doc link that resolved through a removed re-export re-pointed to its
  module.

**Docs / examples converted to `use slither::prelude::*;`**
- `src/compat/rt.rs` — `block_on`'s own doc absorbed the substance of the
  removed lib.rs block (§16.11 attribution + "the one line that pays that
  tax" + the adapter-types-are-not-here clause); prelude pointer replaces the
  crate-root claim; example converted; **`expect()` string** changed
  `slither::block_on` → `slither::prelude::block_on`.
- `src/compat/mod.rs` — three link repairs plus the crate-root paragraph
  rewritten against the prelude.
- `src/compat/io.rs` — `use slither::ReadError;` → `use slither::error::ReadError;`.
- `src/config.rs` — `Config::with_flow_windows` example → prelude.
- `src/identity.rs` — both examples → prelude; two `crate::EndpointBuilder`
  links → `crate::shell::EndpointBuilder`.
- `src/packet/suite.rs` — `channel!`'s example → prelude, plus one sentence
  that the prelude does **not** remove the Cargo.toml `hiss` requirement.
- `src/shell/connection.rs` — `notified()` and `send_message()` examples.
- `src/shell/endpoint.rs` — `connect()` and `EndpointBuilder` examples; two
  `crate::block_on` links; **the LocalSet panic string** and the doc + assert
  of its pinning test (see Open questions).
- `src/shell/mod.rs` — one `crate::block_on` link.
- `src/core/endpoint/mod.rs` — `crate::Config::with_epoch_size` link.
- `src/testutil/mod.rs` — `slither::ConnectionLost` → `slither::error::ConnectionLost`.
- `examples/echo.rs`, `examples/audit_udp.rs`, `examples/bench_vs_tcp.rs` —
  golden-path imports → one prelude line (rand_chacha / `packet::ReferenceSuite`
  kept as module paths); `slither::block_on(` → `block_on(`.
- `demo/runner/src/wire.rs`, `demo/runner/src/sim.rs` — `shell::wire::Wire`
  and `{Config, Identity}` → prelude; `slither::Endpoint::builder()` →
  `Endpoint::builder()`; **testutil imports left on module paths**.
- `demo/runner/src/main.rs` — one comment naming `slither::block_on`.
- `README.md` — quickstart gains `use slither::prelude::*;` as its first
  visible line (still a sketch, not compile-tested); Requirements section's
  `slither::block_on` → `slither::prelude::block_on`.
- `CHANGELOG.md` — the ruling-278 bullet under `[0.2.0] ### Changed`, and the
  pre-existing LocalSet-panic bullet's `slither::block_on` corrected.

**Not in the brief, but required to build (see Open questions #1)**
- 15 `tests/*.rs` + `benches/throughput.rs` — 93 root-path imports rewritten
  to module paths (`slither::error::{…}`, `slither::config::Config`,
  `slither::packet::Handshake`, `slither::shell::Endpoint`, …). `Dir`,
  `StreamId`, `Timestamp` left at the root. Four historical blind-author
  header comments deliberately restored (see Judgment calls).

## Sweeps
### Rule-4 sweep: "crate root" / "re-export" hits

`grep -rn "crate root" src/ README.md CHANGELOG.md` — 6 hits:

| Hit | Disposition |
|---|---|
| `src/lib.rs:319` "[`error`] … Its ten types are re-exported at the crate root." | **FALSE after 278** — rewritten: errors are module-only, `slither::error::…` is the one spelling. |
| `src/lib.rs:364` block_on doc "re-exported at the crate root" | **Removed** with the `pub use compat::block_on;` it documented (substance ported to `compat/rt.rs`). |
| `src/compat/rt.rs:15` "Re-exported at the crate root as [`slither::block_on`](crate::block_on)." | **FALSE after 278** + broken link — rewritten to the prelude path. |
| `src/compat/mod.rs:35` "they are not re-exported at the crate root. [`block_on`] is the exception" | **Stale framing** — rewritten against the prelude. |
| `src/core/mod.rs:67` "`IntroId` is publicly reachable through the crate root" | **Already false at base**, pre-dating 278 (ruling 259(iii) removed the root `IntroId` re-export). Reported, NOT touched — out of 278's scope. See Open questions. |
| `src/packet/suite.rs:212` "a `use slither::hiss` brings no `::hiss` crate root into scope" | **Still true** — this is *hiss's* crate root, not slither's. Unchanged. |

`grep -rni "re-export" src/ README.md CHANGELOG.md` — additional hits beyond the above:

| Hit | Disposition |
|---|---|
| `src/constants.rs:5` "there are no crate-root re-exports and no aliases" | **Still true** (constants were never root re-exported; 278 removes surface, adds none here). Unchanged. |
| `src/lib.rs:117` `hiss::rand_core` re-exports 0.10 | About hiss. Unchanged. |
| `src/lib.rs:372` "adapter **types** are not re-exported here" | Part of the removed block_on doc. Removed with it. |
| `src/lib.rs:388,410,416,419` `SessionId` / Dir-StreamId-Timestamp blocks | **Still true and byte-identical** — 278 explicitly keeps these at the root. Untouched. |
| `src/lib.rs:423,437` `pub use hiss` doc | **Still true** — kept at the root. Untouched. |
| `src/core/mod.rs:68,69,82`, `src/core/connection/mod.rs:182-183`, `src/core/tests.rs:53`, `src/shell/mod.rs:145` | Crate-internal re-export mechanics (E0365, `pub(crate)`). Unaffected by 278. Unchanged. |
| `src/core/mod.rs:79` "re-exported from `lib.rs` beside `ConnectionId`/`IntroId`/`Timestamp`" | **Already stale at base** (259(iii) demoted `ConnectionId`/`IntroId`). Reported, NOT touched. |
| `src/packet/handshake.rs:161`, `src/shell/connection.rs:1265` | `SessionId` is hiss's, re-exported — **still true**, the root keeps it. Unchanged. |
| `src/testutil/capture.rs:40` | About `tracing::Level`, not slither's root. Unchanged. |
| `src/packet/golden_vectors.rs:22,235` | "deliberately not re-exports of `crate::constants`" — unrelated. Unchanged. |
| `README.md:48` | `hiss::rand_core` re-exports 0.10. Unchanged. |
### Import sweep: `use slither::` / `use hiss::noise::{`

After the change, `grep -rn "use slither::" src/ examples/ demo/ README.md`
returns only `use slither::prelude::*;`, `use slither::testutil::…`,
`use slither::constants::…`, `use slither::packet::ReferenceSuite;` and the
one deliberate `use slither::error::ReadError;` (see Judgment calls).

`grep -rn "use hiss::noise::{" src/ examples/` returns two lines, both
correct: `src/prelude.rs:50` (the re-export itself) and
`src/packet/suite.rs:23` (slither's own `ReferenceSuite` definition).

A residual sweep for every demoted name spelled at the root
(`grep -rn "slither::\(block_on\|Config\|Endpoint\|…\)" src/ tests/ benches/
examples/ demo/ README.md CHANGELOG.md`) returns **only** the four restored
historical comments.
### Judgment calls

1. **`src/compat/io.rs`'s `From<ReadError>` example keeps a module path** —
   `use slither::error::ReadError;`. Brief's carve-out: the lesson *is* the
   module path, and 278 makes errors module-only. Not converted to a prelude
   glob (which would not even bring `ReadError` in).
2. **`src/config.rs`'s `with_flow_windows` example WAS converted** to the
   prelude. It is a `Config` example, not a `WallClock` one; `Config` is a
   prelude name, and the old `use slither::Config;` no longer compiles, so the
   choice was prelude vs `slither::config::Config`. The ruling's *"every
   quickstart and example opens with `use slither::prelude::*;`"* decided it.
   `WallClock` docs elsewhere were not touched — `WallClock` is not in the
   prelude and has no root spelling to fix.
3. **`src/packet/suite.rs`'s `channel!` example WAS converted**, and a
   sentence added that the prelude does not remove the Cargo.toml `hiss`
   requirement. Ruling 278 names covering `channel!` as the *reason* the three
   hiss types are in the prelude; leaving the macro's own example on
   `use hiss::noise::{…}` would have been the one place the reason was not
   demonstrated. The "Your crate must depend on `hiss`" section below it is
   unchanged and still carries the manifest rule.
4. **`demo/runner`'s `testutil` imports stay module-path** — per brief;
   testutil is not in the prelude.
5. **Four historical blind-author header comments in `tests/` restored
   verbatim** after a mechanical rewrite touched them:
   - `tests/story_lifecycle.rs:127` — "`slither::Endpoint` / `slither::Connection`
     at the crate root — if the re-export lands under `shell`, prefix `shell::`."
   - `tests/story_dial.rs:70` — same shape, `Connecting`.
   - `tests/story_keepalive.rs:198` — "`slither::Notification` … may well be
     `slither::shell::Notification` … If the re-export lands elsewhere, change
     this one `use` line."
   - `tests/story_mobility.rs:85` — "`slither::Notification` + its three
     variants (§5.3)".
   These sit under headings that read *"Proposed here and ratified nowhere"* /
   *"Names proposed here and nowhere ratified"* / *"Slice-7 names … owned by
   `CONTRACT-7.md`"`. They are a record of what a **blind** author guessed at
   authoring time, not a claim about today's API — and the first three are
   *conditionals whose condition 278 has now satisfied*, so they read
   correctly against the new code (`story_keepalive`'s "change this one `use`
   line" is exactly what happened). Rewriting them would have produced
   "`slither::shell::Endpoint` … at the crate root", which is self-contradicting,
   and would have edited the provenance record. **Flagged for the maintainer**:
   `story_mobility.rs:85` is the one that is a flat statement rather than a
   conditional, and it now names a path that does not exist.
6. **README quickstart kept a display sketch** — one visible `use
   slither::prelude::*;` line added, `slither::block_on(` → `block_on(`. Not
   made compile-tested.

## Sanity runs

```
$ cargo test --all-features 2>&1 | grep "^test result:" | tail -3
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.78s
```
(29 `test result:` lines in total, all `ok`, 0 failed; `grep -cE "FAILED|panicked"` → 0.
The 780-test lib line and the 22 doctests are in there; doctests went 22/22
including the new `src/prelude.rs - prelude (line 3)`.)

```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps 2>&1 | tail -1
   Generated /Users/nicolasdiprima/work/primetype/slither/target/doc/slither/index.html
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features 2>&1 | tail -1
   Generated /Users/nicolasdiprima/work/primetype/slither/target/doc/slither/index.html
```

```
$ cargo fmt --all --check
(no diff)
```

```
$ (cd demo/runner && cargo test 2>&1 | grep "^test result:")
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Also run, not claimed as gates:
- `cargo build --all-features --all-targets` → `Finished dev profile`
- `cargo clippy --all-features --all-targets -- -D warnings` → `Finished dev profile`, zero warnings
- `cargo test` (default features) → all `ok`, 780 + 112 + 11 + 4 + 19 …
- `demo/runner`: `cargo fmt --all --check` clean, `cargo clippy --all-targets -- -D warnings` clean
- `cargo test --all-features --lib build_outside_a_localset` →
  `test shell::endpoint::tests::build_outside_a_localset_panics_with_slithers_own_message ... ok`

## Open questions / anything that looked wrong

1. **The brief's worklist omitted `tests/` and `benches/`, which the change
   breaks.** `grep -rn "use slither::" src/ examples/ demo/ README.md` was the
   stated worklist, but 93 import lines across 15 `tests/*.rs` and
   `benches/throughput.rs` name the removed root re-exports, and the build
   gate fails without them. Converted mechanically to module paths (not to a
   prelude glob — tests name error types constantly, and errors are
   module-only by 278). Reporting rather than silently absorbing, because it
   is a scope difference, not a judgment call.

2. **Ruling 89's `SessionId` doc block could not stay byte-identical.** Its
   last line was `Read it off a live connection with
   [`Connection::session_id`].` — an intra-doc link that resolved **through
   the root `pub use shell::Connection`** this ruling removes. With the
   re-export gone it is an unresolved link and `RUSTDOCFLAGS="-D warnings"`
   fails. Repaired to
   `[`Connection::session_id`](shell::Connection::session_id)` — a link-target
   repair, no reflow and no rewording; the rest of the block, and the whole
   ruling-259(iii) and `pub use hiss` blocks, are untouched. **This is the one
   deviation from the brief's "KEEP BYTE-IDENTICAL" and it was forced by gate
   5.**

3. **Ruling 276's pinned panic wording is invalidated by 278.** The
   `EndpointBuilder::build()` LocalSet guard re-panics naming
   `slither::block_on` — a path that no longer exists — and
   `build_outside_a_localset_panics_with_slithers_own_message` asserts
   `message.contains("slither::block_on")`. Ruling 276 §2 (rulings.md
   ~9078) states the re-panic *"names `slither::block_on` and
   `LocalSet::run_until`"*. Changed the message and the assertion to
   `slither::prelude::block_on`, preserving the intent (name the fix by a
   path that resolves) while changing the exact string the earlier ruling
   quotes. **This wants a line in ruling 278 or a follow-up amendment.** Same
   substitution applied to `compat/rt.rs`'s `expect()` string, README's
   Requirements paragraph, and the CHANGELOG bullet that quoted it.

4. **Two pre-existing stale comments found, NOT touched (outside 278's
   scope).** Both are residue of ruling 259(iii), which demoted
   `ConnectionId`/`IntroId`, and both were already false at the base commit:
   - `src/core/mod.rs:67` — "`IntroId` is publicly reachable through the crate
     root". Verified false: `lib.rs` re-exports only `Dir`/`StreamId`/
     `Timestamp`, and every `IntroId` occurrence outside `src/core/` is a
     private field or a `pub(crate) fn` argument (`shell/staged.rs`,
     `shell/driver.rs`, `shell/shared.rs`) — no public signature names it.
     The `pub` on the re-export is still needed for the E0365 reason the next
     two lines give; only the "crate root" clause is wrong.
   - `src/core/mod.rs:79` — "re-exported from `lib.rs` beside
     `ConnectionId`/`IntroId`/`Timestamp` (ruling 101)". Neither
     `ConnectionId` nor `IntroId` is re-exported from `lib.rs` any more.

5. **`src/constants.rs:5`'s "there are no crate-root re-exports" is now
   *more* true**, not less — confirmed rather than edited.

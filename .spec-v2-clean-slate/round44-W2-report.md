# Round 44 — W2 report (src/lib.rs crate docs restructure)

Base commit: `721167a` (verified as FIRST command:
`git -C /Users/nicolasdiprima/work/primetype/slither log -1 --oneline`
-> `721167a Round 43: the SECV5 pins land — the one-shot keepalive, the
spent-packet separator, the 272 pin`)

Exclusive paths: `src/lib.rs`, `.spec-v2-clean-slate/round44-W2-report.md`.

## 1. Inputs read

## 2. Edit-by-edit checklist (§4.2 – §4.8)

## 3. Verification

### 3.1 `cargo test --doc` (featureless)

### 3.2 `cargo test --doc --all-features`

### 3.3 `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`

### 3.4 `cargo fmt --check`

### 3.5 Staleness greps (plan §10.4)

## 4. Diff stat

## 5. Deviations

- `.spec-v2-clean-slate/round44-D-fix-plan.md` §4.1–§4.8 (the authoritative
  edit list), plus §5 (last row: the duplicated `toml` fence), §6 (the
  example-wins rule), §9 (partition), §10.2/§10.4 (verification).
- `src/lib.rs` @ `721167a`, all 335 lines. `//!` block = lines 1–228;
  separators at 15, 21, 44, 180, 207.
- `examples/echo.rs` (W4, uncommitted, 89 lines) — read, not touched.
- `src/packet/suite.rs` `channel!` (macro accepts `$(#[$meta])*`, so the
  doctest's `/// This application's suite.` is legal), `Identity::public_static`
  (`-> &PublicKeyOf<Self>`), `Connection::{send_message, recv_message, acked,
  close}`, `Endpoint::{accept -> Option<Intro<I>>, connect}`, `compat::block_on`.
- `hiss-0.3.2/src/lib.rs:397` — `pub use rand_core;` exists, so §4.4's
  intra-doc link [`hiss::rand_core`] resolves.

All edits applied by one scripted pass that slices the original `//!` block by
line range and reassembles it in §4.1's order, so the moved blocks cannot be
retyped or reflowed. Post-edit structural proof (`git show 721167a:src/lib.rs`
vs. the new file):

```
obligations body (old 46-179): contiguous & byte-identical at new line 160
shape (old 22-43):             contiguous & byte-identical at new line 136
features (old 208-228):        contiguous & byte-identical at new line 114
authority body (old 16-20):    contiguous & byte-identical at new line 297
opening (old 1-14):            contiguous & byte-identical at new line 1
RATIFIED lines: 3 new / 3 old
old //! lines not in new:
  '//! # The six documentation obligations'                                (§4.6 rename)
  "//! - `core` — §16.4's two sans-io state machines. Crate-internal until the"
  '//!   driver that can drive them exists.'                               (§4.8 de-stale)
  '//! - [`shell`] — the I/O shell. Slice by slice it grows the driver and the'
  '//!   handles; today it carries [`shell::wire::Wire`], the datagram seam an'
  '//!   application supplies.'                                            (§4.8 de-stale)
code-section diff (everything below the //! block):
  -/// hiss = "0.3"
  +/// hiss = { version = "0.3", default-features = false }
```

That is the complete set of deletions: six `//!` lines, all three named by §4,
and one line of the `pub use hiss` doc fence. Nothing else below the `//!`
block moved — the attributes, every `pub use`, both remaining RATIFIED blocks
and the line-242 code comment are untouched.

| Edit | Status |
|---|---|
| §4.1 order (opening · Quickstart · Install · Features · Shape · Before you integrate · The spec is the authority · Modules) | done |
| §4.2 opening kept 1–14 verbatim, two-line `forbid(unsafe_code)` sentence appended to the same paragraph; the `(§6.5, documentation obligation #6)` reference at old line 14 kept | done |
| §4.3 `# Quickstart`, `no_run`, own `fn main`, placed immediately after the opening | done, two deliberate deviations below |
| §4.4 `# Install` added; `# Features` + table + ruling-225 RATIFIED block moved above the obligations byte-for-byte | done |
| §4.5 `# Shape` moved as one block, unedited | done |
| §4.6 heading → `# Before you integrate`; lines 47–179 verbatim | done |
| §4.7 `# The spec is the authority` heading added above the unchanged body, moved to just above `# Modules` | done |
| §4.8 `# Modules` reordered (identity, shell, config, error, packet, constants, compat, testutil, core) with the `shell` and `core` bullets rewritten to §4.8's exact replacement wording | done |
| brief/§5 last row: `lib.rs`'s `toml` fence gains `default-features = false` | done |

### 3.1 `cargo test --doc` (featureless)

```
$ cargo test --doc
   Compiling slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.95s
   Doc-tests slither

running 12 tests
test src/compat/rt.rs - compat::rt::block_on (line 42) - compile ... ok
test src/identity.rs - identity::SoftwareIdentity (line 189) - compile ... ok
test src/lib.rs - (line 175) - compile ... ok
test src/shell/connection.rs - shell::connection::Connection<S>::notified (line 266) - compile ... ok
test src/shell/endpoint.rs - shell::endpoint::EndpointBuilder (line 401) - compile ... ok
test src/lib.rs - (line 25) - compile ... ok
test src/compat/io.rs - compat::io::io::Error (line 105) ... ok
test src/config.rs - config::Config::with_flow_windows (line 270) ... ok
test src/packet/suite.rs - packet::suite::IK (line 275) ... ok
test src/packet/suite.rs - packet::suite::IK (line 311) ... ok
test src/packet/suite.rs - packet::suite::channel (line 177) ... ok
test src/packet/suite.rs - packet::suite::IK (line 243) ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.28s
```

**Count, and whose.** `src/lib.rs` contributes exactly **two**: `(line 175)` is
obligation 1's pre-existing `no_run` (old line 61, moved down by the
restructure) and `(line 25)` is **the one doctest this writer added**, the
§4.3 quickstart. `git show 721167a:src/lib.rs` has exactly one rust fence, so
`lib.rs` went 1 → 2: **+1, and the +1 is mine.**

**A correction to §10.2's baseline, which matters to whoever counts at the
end.** §10.2 records "Baseline at `721167a`: **14 doctests**" without a feature
flag, under the heading *doctests must compile featureless*. 14 is the
**`--all-features`** number, not the featureless one. Featureless at `721167a`
is **11** (the run above is 12 with mine); `--all-features` is **14** (the run
below is 15 with mine) — the difference is the three `testutil` fences
(`testutil::Pair`, `testutil::capture`, `testutil::FlakyPolicy::fail_sends`),
which are behind `test-util`. So the round's expected end state is
**featureless 11 + N** and **all-features 14 + N**, for the same N. A reviewer
who runs the featureless command and compares against 14 will read a correct
tree as short by three.

### 3.2 `cargo test --doc --all-features`

```
$ cargo test --doc --all-features
   Compiling slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.21s
   Doc-tests slither

running 15 tests
test src/identity.rs - identity::SoftwareIdentity (line 189) - compile ... ok
test src/testutil/mod.rs - testutil::Pair (line 1195) - compile ... ok
test src/shell/connection.rs - shell::connection::Connection<S>::notified (line 266) - compile ... ok
test src/compat/rt.rs - compat::rt::block_on (line 42) - compile ... ok
test src/lib.rs - (line 175) - compile ... ok
test src/testutil/capture.rs - testutil::capture (line 18) - compile ... ok
test src/shell/endpoint.rs - shell::endpoint::EndpointBuilder (line 401) - compile ... ok
test src/lib.rs - (line 25) - compile ... ok
test src/packet/suite.rs - packet::suite::IK (line 311) ... ok
test src/packet/suite.rs - packet::suite::IK (line 275) ... ok
test src/compat/io.rs - compat::io::io::Error (line 105) ... ok
test src/config.rs - config::Config::with_flow_windows (line 270) ... ok
test src/packet/suite.rs - packet::suite::channel (line 177) ... ok
test src/testutil/mod.rs - testutil::FlakyPolicy::fail_sends (line 267) ... ok
test src/packet/suite.rs - packet::suite::IK (line 243) ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.98s
```

Both runs were taken while `src/lib.rs` was the only modified source file
(W3's `src/packet/suite.rs` landed afterwards), so the non-`lib.rs` rows above
are `721167a`'s.

### 3.3 `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` (both feature sets)

```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
 Documenting slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.19s
   Generated /Users/nicolasdiprima/work/primetype/slither/target/doc/slither/index.html

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
 Documenting slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.06s
   Generated /Users/nicolasdiprima/work/primetype/slither/target/doc/slither/index.html
```

Zero warnings. The two link risks were the §4.6 rename (no anchor pointed at
the old heading) and §4.4's [`hiss::rand_core`] (resolves — `hiss-0.3.2`
`pub use rand_core;`). `examples/echo.rs` is named in prose, never bracketed.

### 3.4 `cargo fmt --check`

```
$ cargo fmt --check; echo "FMT_EXIT=$?"
FMT_EXIT=0
```

### 3.5 Staleness greps (plan §10.4, the rows that touch `lib.rs`)

```
$ grep -rn "The six documentation obligations" src/           -> exit 1 (no output)
$ grep -rn "Slice by slice it grows" src/lib.rs               -> exit 1 (no output)
$ grep -rn "until the driver that can drive them exists" src/lib.rs
                                                              -> exit 1 (no output)
$ grep -n 'hiss = "0.3"$' src/lib.rs                          -> exit 1 (no output)
$ grep -rnE "crate#|#the-six" src/                            -> exit 1 (no output)  [§10.3]
```

The flagged line-242 comment is intact and is now at 342–346; its wording is
*"until the driver lands"*, which is why the §10.4 grep string does not match
it either way:

```
$ grep -n "until the driver lands" src/lib.rs
343:// outside the crate can drive them until the driver lands, so publishing
```

## 4. Diff stat

```
$ git diff --stat src/lib.rs
 src/lib.rs | 180 +++++++++++++++++++++++++++++++++++++++++++++++--------------
 1 file changed, 140 insertions(+), 40 deletions(-)
```

335 lines → 436. Nothing staged, nothing committed; no file outside
`src/lib.rs` and this report was written.

## 5. Deviations

Two, both inside §4.3's fence, both forced by §6's *"where the two differ, the
example is right"*:

1. **`let answerer_key = *answerer.public_static();`** — §4.3 writes
   `answerer.public_static().clone()`. `public_static()` returns
   `&PublicKeyOf<Self>` (`src/identity.rs:109`), the key is `Copy`, and
   `examples/echo.rs:40` — the artefact that was run — dereferences. `.clone()`
   would also compile; the deref is the example's shape and avoids
   `clippy::clone_on_copy` if doctests are ever linted.
2. **The `examples/echo.rs` sentence is prose, not a link, and says what the
   example adds.** §4.3's draft reads *"The same program is
   [`examples/echo.rs`]"*; §4.3 note (a) forbids the bracket pair, and the
   landed example is not the *same* program — it echoes back and the dialler
   also calls `recv_message`. Written as: *"The same program, with an echo back
   and comments, is `examples/echo.rs` in the repository — run it with
   `cargo run --example echo`."* No brackets, no URL (the repo is not public
   yet, and §4.3 note (a) makes the URL conditional on that).

Everything else in §4.3 is the plan's text character-for-character, including
the `.unwrap()` style, the `assert_eq!(msg, b"hello")` (verified to compile:
`recv_message` returns `Vec<u8>`), the step numbering and the
`"Before you integrate" #6` cross-reference — which the §4.6 rename makes
correct.

**A note on §4.2's appended sentence — checked, and it holds.** It claims
*"no RustCrypto crate appears in the graph"*. Verified rather than assumed:

```
$ cargo tree --all-features -e normal | grep -iE 'x25519|curve25519|sha2|aes|chacha20|poly1305|digest|elliptic-curve|crypto-common|generic-array|typenum|universal-hash' | sort -u
(no output)
$ grep -n hiss Cargo.toml
101:hiss = { version = "0.3.2", default-features = false }
```

slither's own manifest already carries `default-features = false` (line 101,
with the comment naming `x25519-cryptoxide` as the default backend it excludes),
so the graph is clean and the sentence is true. **The `default-features = false`
fix in this round is about the *consumer's* graph, not slither's**: the two
`toml` fences told a consumer to write `hiss = "0.3"`, and agent A followed that
line into `x25519-cryptoxide` — which is a `cryptoxide` crate, **not** a
RustCrypto one, so it never falsified this sentence. It was the wrong backend,
not the wrong crypto family. Recording that distinction because the fence fix
and the appended sentence look like the same fact and are not.

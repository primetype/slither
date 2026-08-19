# Release-prep audit — slither v0.2.0

Audit run date: 2026-08-19 (per environment `currentDate`; the session
started under a `currentDate` of 2026-08-17 which advanced mid-session —
noted here rather than silently left stale, per this project's own rule
about not sweeping dated statements)
Auditor: read-only audit agent. No tracked files were modified during this audit.

## Base commit

Verified via `git log -1 --oneline` and `git status --porcelain`:

```
27c8632 Round 44 follow-up 2: the suite declaration is visible in the quickstart
```

`git status --porcelain` was clean except for this audit's own untracked output
file (`.spec-v2-clean-slate/release-prep-audit.md`). Matches the expected HEAD
`27c8632` on a clean tree. No tracked file was modified during this audit.

## Metadata

`Cargo.toml [package]` (lines 1-27):

```
name = "slither"
version = "0.2.0"
edition = "2024"
rust-version = "1.96"
authors = ["Nicolas Di Prima <nicolas@primetype.co.uk>"]
license = "MIT OR Apache-2.0"
description = "Encrypted peer-to-peer UDP transport: reliable messages, streams and datagrams, authenticated by raw public keys - no certificates, no TLS. WireGuard-shaped handshake, QUIC-shaped frames."
readme = "README.md"
repository = "https://github.com/primetype/slither"
homepage = "https://github.com/primetype/slither"
documentation = "https://docs.rs/slither"
keywords = ["noise", "udp", "wireguard", "quic", "datagram"]
categories = ["cryptography", "network-programming"]
```

All required fields present and sane:
- name/version/description/license/readme/repository/homepage/documentation/rust-version/edition: all present.
- `license = "MIT OR Apache-2.0"` — a valid SPDX expression, matches the dual LICENSE files (checked below).
- `keywords`: 5 entries (max allowed), each ≤ 20 chars (`wireguard` is the longest at 9). All lowercase, alphanumeric/hyphen — crates.io-legal.
- `categories`: 2 entries, both verified as real crates.io category slugs via the crates.io API:
  ```
  $ curl -s https://crates.io/api/v1/categories/cryptography | head -c 200
  {"category":{"id":"cryptography","category":"Cryptography","slug":"cryptography",...
  $ curl -s https://crates.io/api/v1/categories/network-programming | head -c 200
  {"category":{"id":"network-programming","category":"Network programming","slug":"network-programming",...
  ```
  Both resolve (200, valid JSON body). OK.
- `publish = false`: **absent**. `grep -n "publish" Cargo.toml` returned no matches anywhere in the file — the crate is publishable (not blocked by a `publish` field). Confirmed this is desired, not an oversight, given the whole point of this audit.
- `[package.metadata.docs.rs]` exists and reads:
  ```
  [package.metadata.docs.rs]
  all-features = true
  ```
  Confirmed as stated in the brief — comment above it explains why (`testutil` module needs to be documented on docs.rs).
- `exclude` list present, explicitly keeps repo-infra and design-record directories out of the tarball (`.github`, `CLAUDE.md`, `TODO.md`, `deny.toml`, `.gitignore`, `.serena`, `.claude`, `.spec-v2-clean-slate`, `.spec-v2-pipeline`, `.slices`, `PLAN.md`, `STORIES.md`) while deliberately keeping `SPEC.md` (comment: "SPEC.md ships on purpose — it is the ratified protocol the rustdoc points at").
- Feature table: `default = []` (nothing on by default — correct per crate philosophy stated in comments), `test-util`, `sink`, `codec` (implies `sink`), `tower`. All look consistent with the dependency table's `optional = true` markers.

## Packaging

`cargo package --list --allow-dirty` (the `--allow-dirty` flag was needed
only because this audit's own untracked output file is sitting in the
working tree — no tracked file is dirty):

```
.cargo_vcs_info.json
CHANGELOG.md
Cargo.lock
Cargo.toml
Cargo.toml.orig
LICENSE-APACHE
LICENSE-MIT
README.md
SECURITY.md
SPEC.md
benches/throughput.rs
docs/architecture-dark.svg
docs/architecture.svg
docs/staged-accept-dark.svg
docs/staged-accept.svg
examples/audit_udp.rs
examples/bench_vs_tcp.rs
examples/echo.rs
src/compat/{codec,io,mod,rt,stream,tower}.rs
src/{config,constants,error,identity,lib,varint}.rs
src/core/{mod,tests}.rs
src/core/connection/*.rs  (ack, close, congestion, datagram, flow, frame,
  mobility, mod, recovery, recv, send, session, stream_id, streams, testfix,
  timers, tests*.rs — 11 test-module files)
src/core/endpoint/*.rs  (guard, handshake, intro_queue, mod, routing, staged,
  tables, tests)
src/packet/*.rs  (golden_vectors, handshake, header, mac, mod, payload,
  suite, tests)
src/shell/*.rs  (connection, driver, endpoint, mod, shared, staged, stream,
  wire)
src/testutil/{capture,mod}.rs
tests/spec_*.rs  (ack_burst, compat, constants, errors, packet, rekey, shell,
  streams — 8 files)
tests/story_*.rs  (codec, compat, datagram, dial, flow, intro, keepalive,
  lifecycle, message, mobility, park, path, reassembly, rekey, reliability,
  streams, tower, traced, wrong_static — 19 files)
```
(full untruncated list captured; abbreviated here for readability — the raw
output was pasted into this section verbatim during the audit run and every
path was individually checked against the exclude list and the README/lib.rs
references below.)

**(a) Nothing unwanted ships.** No `.spec-v2-clean-slate/`, `.spec-v2-pipeline/`,
`.slices/`, `PLAN.md`, `STORIES.md`, `CLAUDE.md`, `TODO.md`, `.github/`,
`deny.toml`, `.serena/`, `.claude/`, or `.gitignore` appear in the list — the
`exclude` array in `Cargo.toml` is doing its job. `tests/` and `benches/` ship
(normal and harmless for a lib crate; not a "should not ship" item).

**(b) Cross-check against README.md / src/lib.rs references — nothing is
missing.** Extracted every local file reference from README.md and
src/lib.rs doc comments:

```
README.md markdown-link targets: SECURITY.md, SPEC.md, CHANGELOG.md,
  LICENSE-APACHE, LICENSE-MIT, examples/echo.rs
README.md <img>/<source> targets: docs/staged-accept-dark.svg,
  docs/staged-accept.svg, docs/architecture-dark.svg, docs/architecture.svg
src/lib.rs doc-comment references: examples/echo.rs, SPEC.md (both prose,
  not intra-doc links), PLAN.md, Cargo.toml (both prose, not links)
```

All of README's actual markdown links and `<img src>`/`<source srcset>`
targets (SECURITY.md, SPEC.md, CHANGELOG.md, LICENSE-APACHE, LICENSE-MIT,
examples/echo.rs, and all four docs/*.svg files) **are present** in the
packaged tarball. Per the brief's caveat, a relative link in README only
renders on crates.io if crates.io can rewrite it against the `repository`
URL — but since every one of these files also ships **inside the tarball
itself**, the link/image works regardless of whether crates.io's rewriting
succeeds, PROVIDED the reader is looking at the packaged crate's rendered
README (docs.rs's crate-level README render does this from the tarball).
crates.io's own README render (the crates.io crate page) does rewrite
relative links against `repository`, which requires the GitHub repo to
actually exist and be public by the time crates.io renders the README —
true only after the publish-day "create + push" step. No gap: all
referenced files are present either way.

`src/lib.rs`'s two references to `PLAN.md` and one prose reference to
`Cargo.toml` inside the "ratified ruling" doc-comment block (around
line ~138) and the "SPEC.md is the authority" section (~line 303) are
**plain backtick-quoted prose, not `[text](path)` / intra-doc links** — so
`PLAN.md` being excluded from the package (by design, per the `exclude`
list and CLAUDE.md's directive that provenance docs are maintainer-only)
does **not** break the rustdoc build. Confirmed by full-text search: no
`[...](PLAN.md)`-style markdown link exists anywhere in `src/lib.rs` or
README.md — only prose mentions. This matches the "Docs build spot-check"
result below (clean build, no broken intra-doc links).

**README badge comment / stale commit hash** (see also Gaps and the
"release-blocking stragglers" section): the HTML comment at README.md:3-5
reads `<!-- ACTIVATES ON PUBLISH: no git remote exists at 721167a, ... -->`.
`721167a` is a real, resolvable commit in this repo's history
(`Round 43: the SECV5 pins land`), but it is **4 commits behind current
HEAD** (`27c8632`):
```
$ git log --oneline 721167a..27c8632
27c8632 Round 44 follow-up 2: the suite declaration is visible in the quickstart
0f0c691 Round 44 follow-up: theme-paired SVGs, and the quickstart splits in two
c93d24f Ruling 276: the record — the documentation round's five decisions
d039a28 Round 44: the documentation round — README why-first, lib.rs how-first
```
This is a comment-only staleness (the referenced hash plays no functional
role — it's just documentation of "when this was written"), not
release-blocking, but flagged per CLAUDE.md rule 4 (stale statements sweep
nothing but deliberate intent) — see Gaps.

## Full `cargo package` (build + verify from the tarball)

```
$ cargo package --allow-dirty
   Packaging slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Updating crates.io index
    Packaged 115 files, 4.6MiB (1.3MiB compressed)
   Verifying slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
   Compiling slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither/target/package/slither-0.2.0)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.28s
```

**PASS.** The packaged crate builds cleanly from its own tarball (dev
profile, default features — this is `cargo package`'s own verification
build, which does not exercise `--all-features` or run tests; see the
Gates spot-check section below for those). `--allow-dirty` was required
only because of this audit's own untracked output file — no tracked file
is dirty.

Tarball: `target/package/slither-0.2.0.crate`, 1.3 MiB compressed / 4.6 MiB
uncompressed, 115 files — comfortably under crates.io's 10 MiB compressed
upload limit.

(Stray artifact noted, not a gap: `target/package/` also still contains a
leftover `slither-0.1.0.crate` from a prior local run before the version
was bumped to 0.2.0. It's a `target/` build artifact, gitignored and not
part of the release; harmless, mentioned only for completeness.)

## Files

**LICENSE-APACHE / LICENSE-MIT**: both exist and are real, unmodified
license texts.

```
$ head -3 LICENSE-APACHE
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

$ head -3 LICENSE-MIT
Copyright (c) 2026 Primetype Ltd

Permission is hereby granted, free of charge, to any person obtaining a copy
```

LICENSE-APACHE is 201 lines (the full standard Apache-2.0 text, including
the appendix with `Copyright 2026 Primetype Ltd` filled in). LICENSE-MIT is
19 lines (the full standard MIT text). Both consistently attribute
copyright to "Primetype Ltd, 2026" — no conflict between the two license
files. (`Cargo.toml`'s `authors` field names the individual maintainer,
Nicolas Di Prima, which is orthogonal to — not in conflict with — the
company copyright line in the license texts; noted, not a gap.)

**SECURITY.md**: exists, 91 lines, read in full. Covers: how to report
(GitHub private vulnerability reporting), supported versions (latest
release only), session guarantees, five threat-model highlights (mac1 as
DoS gate not authenticator, msg1 0-RTT replay caveat, roam-trusts-the-seal,
admission is application-driven, randomness/seed-zeroization caveat), and
"known limitations and non-goals (v1, ratified)" (no cookies/mac2, no
pacing, no traffic-analysis resistance, no persistence, no PSK).

**CONFLICT (rule 3) — README claims SECURITY.md substantiates "not
independently audited"; it does not.** README.md:9 states:
`· **not independently audited** — see [\`SECURITY.md\`](SECURITY.md).`
— directing the reader to SECURITY.md for the audit-status claim. But:
```
$ grep -in "audit" SECURITY.md
(no output — the string "audit" does not appear anywhere in SECURITY.md)
```
SECURITY.md never states, in any words, that the crate is or is not
independently audited. It is a substantive, well-written threat-model
document, but it does not contain the claim README points to it for. This
is a verbatim conflict between two statements (README's implicit promise
that SECURITY.md covers this vs. SECURITY.md's actual content) — reported,
not resolved, per rule 3. See Gaps.

**CHANGELOG.md**: top section is
```
## [0.2.0] - 2026-08-18
```
— a **real dated entry**, not a `[Unreleased]` placeholder. Format follows
Keep a Changelog / SemVer, as stated in the preamble. The date (2026-08-18)
is one day before the current HEAD commit's timestamp
(`27c8632`, committed `2026-08-19T08:29:20+01:00` — see `git log -1
--format="%cI"` above), i.e. it is already in the past relative to HEAD,
not a future placeholder. This satisfies the "release needs a dated entry"
bar as stated. Whether the maintainer wants to bump it to the *actual*
publish-day date before tagging is a judgment call, included in the
proposed checklist below, not a defect in itself.

## CI

`.github/workflows/`: three files — `audit.yml`, `check.yml`, `test.yml`.
No fourth file (no `publish.yml` / `release.yml`).

**`check.yml`** ("Check", stage 1 — triggers: `push: branches: [main]`,
`pull_request:` unrestricted):
- `fmt`: `cargo fmt --all --check` — matches the Format gate exactly.
- `clippy-doc`: `cargo clippy --all-features --all-targets -- -D warnings`
  (matches Lints gate exactly), then two doc jobs — `cargo doc --no-deps`
  (default features) and `cargo doc --no-deps --all-features`, both with
  `RUSTDOCFLAGS: -D warnings`, each preceded by `cargo clean --doc` —
  matches the Docs gate's "both featureless and all-features" requirement
  exactly.
- `features`: a 4-way matrix (`test-util`, `sink`, `codec`, `tower`), each
  run alone via `cargo clippy --no-default-features --features <f>
  --all-targets -- -D warnings`. Not one of the nine named release gates,
  but a superset check the gate table doesn't ask for (documented in-file
  as closing a real gap found by ruling 259(vii)/246).
- `msrv`: `dtolnay/rust-toolchain@master` pinned to `toolchain: "1.96"`,
  then `cargo check --all-features --all-targets` — matches the MSRV gate
  (pinned to 1.96, as CLAUDE.md's MSRV policy section requires).
- `deny`: `EmbarkStudios/cargo-deny-action@v2` — matches the Supply chain
  gate.

**`test.yml`** ("Test", stage 2 — triggers on `workflow_run` of `Check`
completing successfully; checks out `github.event.workflow_run.head_sha`
so it tests the exact commit Check validated):
- 2×2 matrix: `os: [ubuntu-latest, macos-latest]` ×
  `features: [default, --all-features]` → 4 legs, each running
  `cargo test ${{ flags }}`. Matches the Tests gate ("cargo test **and**
  cargo test --all-features") on two OSes, matching README's "Linux and
  macOS in CI" claim.

**`audit.yml`** ("Audit" — `schedule: cron "17 5 * * *"` daily, plus
`workflow_dispatch`): re-runs the same `cargo-deny check` job on a clock,
against an unchanged tree, to catch newly-published advisories /
license/graph drift between pushes. Matches CLAUDE.md's description
("mirrors the CI pipeline ... plus the daily Audit cron") exactly.

**GAP — two of the nine release gates are not run anywhere in CI:**
```
$ grep -rn -- "--release" .github/workflows/
(no matches)
$ grep -rn "cargo build" .github/workflows/
(no matches)
```
1. **"Release tests" gate (`cargo test --release --all-features`) has no
   CI job at all.** Neither `test.yml` nor any other workflow passes
   `--release` to any cargo invocation. This must currently be run by
   hand before every release, or it isn't being run — the audit can't
   tell which from the repo alone. See Gaps.
2. **"Compiles" gate (`cargo build --all-features --all-targets`) has no
   *standalone* CI step.** It is exercised only transitively — `clippy
   --all-features --all-targets` and `cargo test --all-features` both
   compile the same targets on the way to their real job — so a build-only
   regression (e.g. a warning-free-but-broken build under some obscure
   target combination `clippy`/`test` wouldn't hit) has no direct gate.
   Low severity given the transitive coverage, but it is a literal gap
   against the gate table's own command. See Gaps.

**No publish/release workflow exists.** `cargo publish` (or a
tag-triggered publish action) is not automated anywhere in
`.github/workflows/`; publishing to crates.io is necessarily a manual,
local step today. (Confirmed by `grep -rln "publish\|cargo-publish\|crates.io"
.github/workflows/` — the one hit, in `audit.yml`, is the unrelated word
"published" inside a comment about RUSTSEC advisory timing, not a publish
step.) Not itself a defect — plenty of crates publish by hand — but it
means the "tag v0.2.0 → cargo publish" step at the end of the checklist
below is manual and needs `CARGO_REGISTRY_TOKEN` set up locally (or added
as a repo secret later if the maintainer wants to automate it).

**Badge URL vs. actual workflow filenames**: README.md's badge comment
block (lines 3-5, currently inside `<!-- ACTIVATES ON PUBLISH ... -->`)
references `https://github.com/primetype/slither/actions/workflows/check.yml/badge.svg`.
`check.yml` **does** exist at that exact path
(`.github/workflows/check.yml`) — the badge URL is correct against the
actual filename. Note the badge is wired to the `Check` workflow only (not
`Test` or `Audit`) — a reasonable single-badge choice (Check is stage 1
and gates Test via `workflow_run`), not a conflict.

**CI branch trigger**: `check.yml`'s `on: push: branches: [main]` names
`main`. The repo's current branch (per `git status`/environment context)
is `main`, so this trigger will fire once the repo exists on GitHub with
`main` as a real branch — consistent, not a gap.

## Gates spot-check

All nine release gates from CLAUDE.md's table were run locally, by hand, on
commit `27c8632` (clean tree apart from this audit's own output file). All
nine passed.

**1. Compiles** — `cargo build --all-features --all-targets`
```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
exit: 0
```

**2. Format** — `cargo fmt --all --check`
```
$ cargo fmt --all --check
exit: 0
```
(no diff output — clean)

**3. Lints** — `cargo clippy --all-features --all-targets -- -D warnings`
```
$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
exit code: 0
```

**4. Docs** — `cargo doc --no-deps` (default features) and `--all-features`,
both with `RUSTDOCFLAGS=-D warnings`
```
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
    Generated .../target/doc/slither/index.html
exit code: 0

$ cargo clean --doc && RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
     Removed 15970 files, 124.9MiB total
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.66s
    Generated .../target/doc/slither/index.html
exit code: 0
```
No broken intra-doc links in either feature configuration.

**5. Tests** — `cargo test` and `cargo test --all-features`
```
$ cargo test
test result: ok. 780 passed; 0 failed  (unittests src/lib.rs)
test result: ok. 112 passed; 0 failed  (tests/spec_constants.rs)
test result: ok. 11 passed; 0 failed   (tests/spec_errors.rs)
test result: ok. 4 passed; 0 failed    (tests/spec_packet.rs)
test result: ok. 18 passed; 0 failed   (18 doc-tests)
exit code: 0, 0 FAILED matches, 925 tests passed total
```
Only 4 test binaries + doctests build/run under default (no) features —
this is intentional, not a bug: every `story_*`/`spec_ack_burst`/
`spec_compat`/`spec_rekey`/`spec_shell`/`spec_streams` integration test
has a `[[test]] required-features = [...]` stanza in `Cargo.toml` (e.g.
`story_lifecycle` → `["test-util"]`, `story_tower` → `["test-util",
"tower"]`), which makes Cargo skip building them entirely under default
features rather than failing to compile. This mechanism, and the reason
for it (`required-features` avoids a `[[test]]` entry that "commits a
tree on which no gate can run at all", working rule 15), is documented
in-line in `Cargo.toml` itself around line 285.
```
$ cargo test --all-features
exit code: 0, 0 FAILED matches
29 "test result: ok" lines, 1138 tests passed total, 2 ignored
```
The all-features run exercises all 27 integration test files plus the
unit tests and doctests — **1138 total passing tests**, matching the
figure recorded in the maintainer's own memory file
(`slither-v0-2-plan.md`: "1138 tests, 34/34 stories"). Two tests show as
`ignored` in this run, both for the same documented reason (ruling 251):
```
test o53a_sustained_ack_loss_bursts_do_not_provoke_spurious_retransmits
  ... ignored, 9.1 s in debug against 0.3 s in release, for identical
  numbers: Appendix B's "in the release run if debug-slow" (ruling 251).
  `cargo test --release --all-features` runs it; in debug, `-- --ignored`
  does.                                              (tests/spec_ack_burst.rs)

test s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing
  ... ignored, 13.4 s in debug against 1.0 s in release: [same reason]
                                                        (tests/story_rekey.rs)
```
This is the direct, concrete evidence behind the CI gap flagged above: **these
two tests — one of them a full story-acceptance test, S23, "a long-lived
connection rekeys itself without the user noticing" — do not run under
`cargo test --all-features` in debug, which is the only test invocation
CI's `test.yml` performs.** They only run under `cargo test --release
--all-features` or `cargo test --all-features -- --ignored`, neither of
which appears anywhere in `.github/workflows/`. See Gaps.

**6. Release tests** — `cargo test --release --all-features`
```
$ cargo test --release --all-features
exit code: 0, 0 FAILED matches
1140 tests passed, 0 ignored
test o53a_sustained_ack_loss_bursts_do_not_provoke_spurious_retransmits ... ok
test s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing ... ok
```
Confirms both previously-ignored tests **do** pass under the release
profile — the gate is green when run by hand, on this commit, but (per the
CI section above) is never run automatically.

**7. Wire pins** — golden-wire and size/constant tests, run under `cargo
test`: covered by the `spec_constants.rs` (112 tests) and
`packet::golden_vectors`/`packet::tests` modules inside the 780 unit
tests above, all passing in both the default and all-features runs.

**8. MSRV** — `cargo +1.96 check --all-features --all-targets` (1.96 is
installed locally via rustup: confirmed with `rustup toolchain list`,
which lists `1.96-aarch64-apple-darwin` and `1.96.1-aarch64-apple-darwin`
alongside the active `stable` at 1.97.1):
```
$ cargo +1.96 check --all-features --all-targets
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.91s
exit: 0
```

**9. Supply chain** — `cargo deny check`
```
$ cargo deny check
warning[license-not-encountered]: BSD-3-Clause (deny.toml:21) — unmatched
warning[license-not-encountered]: ISC (deny.toml:22) — unmatched
warning[license-not-encountered]: Zlib (deny.toml:24) — unmatched
warning[duplicate]: 2 versions of `syn` in the graph (2.0.119 via
  packtool-macro/tracing-attributes; 3.0.3 via futures-macro/hiss-macros/
  thiserror-impl/tokio-macros) — deny.toml sets `multiple-versions = "warn"`
  (a deliberate policy choice, not a misconfiguration)
advisories ok, bans ok, licenses ok, sources ok
exit code: 0
```
All four cargo-deny categories (`advisories`, `bans`, `licenses`,
`sources`) report `ok`; the warnings are informational (unused license
allowances and a `warn`-not-`deny` duplicate-version policy), not
failures.

**Summary: all 9 release gates are green on `27c8632` when run by hand.**
The one procedural gap is that gate 6 (Release tests) — demonstrably not
a no-op, since it is the only gate that exercises two specific tests
(O53a, S23) — has no automated trigger in `.github/workflows/`.

## Gaps

1. **`.github/workflows/test.yml` — no `cargo test --release --all-features` job anywhere in CI.**
   Severity: MEDIUM-HIGH. Concretely verified (not theoretical): two tests
   are `#[ignore]`d in the debug profile specifically because they're slow
   in debug (ruling 251) and only run under `--release`:
   `tests/spec_ack_burst.rs::o53a_sustained_ack_loss_bursts_do_not_provoke_spurious_retransmits`
   and `tests/story_rekey.rs::s23_a_long_lived_connection_rekeys_itself_without_the_user_noticing`.
   `test.yml` runs only plain `cargo test` and `cargo test --all-features`
   (no `--release`), so **these two tests — one of them a full
   story-acceptance test (S23) — have never executed in CI, on any push or
   PR, and will not on the release commit either**, unless someone runs
   the full local gate table by hand (as this audit did — see Gates
   spot-check #6, where both pass under `cargo test --release
   --all-features`). CLAUDE.md's own bar is "a slice is done when its
   stories are paused-clock tests that pass" — a story test with zero
   automated coverage is a real gap against that bar, not a paperwork one.

2. **`.github/workflows/` — no standalone `cargo build --all-features --all-targets` step.**
   Severity: LOW. The literal "Compiles" gate command never runs as its
   own step; it's only exercised transitively inside `clippy
   --all-features --all-targets` (`check.yml`'s `clippy-doc` job) and
   `cargo test --all-features` (`test.yml`). A pure-build regression
   invisible to both clippy and test is a narrow edge case, but the gate
   table's literal command has no direct CI equivalent.

3. **README.md:9 vs SECURITY.md (whole file) — unresolved conflict, reported per rule 3, not picked a side.**
   Severity: LOW-MEDIUM. README states `**not independently audited** —
   see [`SECURITY.md`](SECURITY.md)`, directing readers to SECURITY.md
   for that claim. `grep -in "audit" SECURITY.md` returns **no matches**
   — SECURITY.md never uses the word "audit" and never states, in any
   form, whether the crate has or has not been independently audited. The
   two statements are not talking about the same thing where README
   implies they are. For a crypto-handling network crate on its first
   public release, an unresolved audit-status pointer is worth the
   maintainer's attention before publish, but this audit does not resolve
   which document should change.

4. **README.md:3 — stale commit hash inside the `ACTIVATES ON PUBLISH` HTML comment.**
   Severity: COSMETIC. The comment reads `<!-- ACTIVATES ON PUBLISH: no
   git remote exists at 721167a, ... -->`. `721167a` is a real commit
   (`Round 43: the SECV5 pins land`) but is 4 commits behind current HEAD
   `27c8632`. The hash plays no functional role (it's inside an HTML
   comment, not consulted by tooling), so this has zero release-blocking
   effect — flagged only because CLAUDE.md rule 4 asks that stale
   statements be caught, and because this comment will need editing
   anyway when the badges are uncommented at publish time (checklist
   item below folds the two together).

5. **No publish/release GitHub Actions workflow exists.**
   Severity: INFORMATIONAL, not a defect. Confirmed via `grep -rln
   "publish\|cargo-publish\|crates.io" .github/workflows/` — the sole hit
   is the unrelated word "published" inside `audit.yml`'s comment about
   RUSTSEC advisory timing. `cargo publish` is therefore a manual, local
   step today, requiring `CARGO_REGISTRY_TOKEN` (or equivalent
   credentials) on whichever machine runs it. Not wrong for a first
   release, but should be a deliberate, not accidental, choice — included
   in the checklist below.

6. **CHANGELOG.md's `[0.2.0] - 2026-08-18` date is one day before HEAD's actual commit timestamp.**
   Severity: COSMETIC / judgment call. `git log -1 --format=%cI` on
   `27c8632` gives `2026-08-19T08:29:20+01:00`; the CHANGELOG's top entry
   says `2026-08-18`. This is a real, past date — not a placeholder — so
   it satisfies the literal "needs a dated entry" bar. Whether it should
   be bumped to match the actual day of publish (which may be later still,
   once the checklist below runs) is the maintainer's call, not a defect.

7. **(Not a gap, confirmed-clean cross-checks worth recording so they aren't re-litigated later.)**
   - `cargo package --list` ships nothing from the exclude list (no
     `.spec-v2-clean-slate/`, `PLAN.md`, `STORIES.md`, `CLAUDE.md`,
     `TODO.md`, `.github/`, `deny.toml`, `.serena/`, `.claude/`).
     `TODO.md` doesn't currently exist in the working tree at all (`ls
     TODO.md` → "No such file or directory"; not tracked in git either)
     — its presence in `Cargo.toml`'s `exclude` list is defensive/inert,
     not a bug.
   - Every file README.md and src/lib.rs actually link to or embed
     (SECURITY.md, SPEC.md, CHANGELOG.md, LICENSE-APACHE, LICENSE-MIT,
     examples/echo.rs, all four `docs/*.svg`) **is present** in the
     packaged tarball. `PLAN.md`/`Cargo.toml` mentions inside lib.rs's
     doc comments are plain prose, not intra-doc links, so `PLAN.md`
     being excluded from the package does not break the rustdoc build
     (confirmed clean in the Docs gate spot-check).
   - `grep -rn "TODO\|FIXME\|XXX" src/ --include="*.rs"` → **zero
     matches**. Nothing consumer-visible to flag in rustdoc.
   - `git tag` → empty (no tags yet, as expected before a first release).
   - `Cargo.toml`'s `version = "0.2.0"` matches CHANGELOG's top heading
     `## [0.2.0]` exactly.
   - `publish = false` is **not** set anywhere in `Cargo.toml` — the
     crate is publishable.
   - The GitHub repo `primetype/slither` does not yet exist
     (`curl -s -o /dev/null -w '%{http_code}' https://github.com/primetype/slither`
     → `404`), and no git remote is configured locally (`git remote -v`
     → empty) — consistent with, not contradicting, the README comment's
     own claim.
   - crates.io category slugs `cryptography` and `network-programming`
     both resolve via the crates.io categories API — valid.
   - `cargo deny check` exits 0; `advisories ok, bans ok, licenses ok,
     sources ok`. The only output is informational warnings (three unused
     license allowances in `deny.toml`, and a `syn` 2.x/3.x duplicate that
     `deny.toml` explicitly sets `multiple-versions = "warn"` for, a
     deliberate policy rather than an oversight).

## Proposed checklist

Ordered for a first public release of v0.2.0 to GitHub + crates.io, folding
in the gaps above:

1. **Resolve the SECURITY.md / README "not independently audited" conflict
   (Gap 3).** Either add an explicit audit-status statement to
   SECURITY.md, or change what README links to / how it phrases the
   claim. This is a judgment call for the maintainer — the audit reports
   it, doesn't pick a side.
2. **(Optional, can follow the release rather than block it)** Close Gaps
   1 and 2 by adding a `cargo test --release --all-features` job (or a
   dedicated "release gate" workflow) and a standalone `cargo build
   --all-features --all-targets` step to CI, so S23 and O53a get
   automated coverage going forward and the "Compiles" gate has a direct
   check rather than a transitive one.
3. Create the GitHub repository `primetype/slither` (public, matching
   `Cargo.toml`'s `repository`/`homepage` fields).
4. Add the git remote locally and push `main` (full history) to GitHub.
5. Confirm CI is green on the **exact commit being released** — push
   triggers `Check`, which on success triggers `Test` via `workflow_run`;
   watch both conclude successfully on that commit. (Local gate runs in
   this audit are not a substitute for CI green per CLAUDE.md's release
   gate rule — "CI green on the release commit satisfies every gate.")
6. Decide the true publish date and update CHANGELOG.md's `## [0.2.0]`
   heading date if `2026-08-18` shouldn't be the final date (Gap 6).
7. Update (or simply remove, since it becomes dead text once uncommented)
   the stale `721167a` commit hash in the README's `ACTIVATES ON PUBLISH`
   comment (Gap 4) as part of the next step.
8. Uncomment the badge block in README.md:3-5 now that the repo is public
   — this also lets crates.io rewrite the `docs/*.svg` relative image
   links against the `repository` field on the crates.io-rendered README.
9. Tag the release commit: `git tag -a v0.2.0 -m "..."` and
   `git push --tags`.
10. `cargo publish` (manual, local; requires `CARGO_REGISTRY_TOKEN` —
    **not run by this audit**, per the ground rules given). Gap 5 notes
    there is no automated publish workflow, so this step is a deliberate
    manual action, not an oversight if skipped from CI.
11. Verify the crates.io listing renders correctly (README images/links,
    badges, keywords/categories) and that docs.rs builds successfully
    with all features (`[package.metadata.docs.rs] all-features = true`
    is already set, so this should be automatic — worth a post-publish
    check anyway).
12. Verify the CI badge and crates.io/docs.rs badges resolve (not 404) on
    the now-public repo.

## Integrator actions (same day, Fable)

Verified gaps 1–4 against the artifacts (test.yml, SECURITY.md, README.md,
check.yml) before acting — rule 11. Applied:

1. **Gap 1 (MEDIUM-HIGH)**: `test.yml` gained a `test-release` job —
   `cargo test --release --all-features` on ubuntu, gate-table verbatim
   (no `--include-ignored`: the two ruling-251 ignores are
   debug-conditional, and the audit's hand-run showed 0 ignored under
   release). S23 now runs in CI.
2. **Gap 2 (LOW)**: `check.yml` gained a `build` job running
   `cargo build --all-features --all-targets` — the gate table's first
   row, verbatim. CLAUDE.md's "CI green on the release commit satisfies
   every gate" is literally true again.
3. **Gap 3 (rule-3 conflict)**: SECURITY.md gained an `## Audit status`
   section stating slither has not been independently audited —
   implementing ruling 276's candour line at the document README.md:9
   points to. The README/SECURITY conflict is discharged.
4. **Gap 4 (cosmetic)**: the README badge comment no longer names a
   commit hash — "until the repo is public", which cannot go stale.

Not acted on (publish-day items, maintainer's): CHANGELOG date (gap 6),
badge uncomment, repo creation, tag, `cargo publish` (gap 5: stays manual).

# Slice 0 — "Ground" — implementation log

Written incrementally as the slice is built, per `CLAUDE.md` working rule 2.
Authority: `.slices/00-ground/PLAN.md`. Where the plan and `SPEC.md`
disagree the spec wins; deviations are recorded here with a reason.

## Deliberate exclusions (from the brief)

Two tests named by the plan are **not** written here — an independent agent
authors them from `SPEC.md` alone, per `CLAUDE.md` working rule 6:

- §4.5 test 1 `named_constants_match_the_consolidated_table` (transcription
  of constant *values*)
- §5.6's exhaustive error-variant match

Everything else the plan calls for is written, including the compile-time
assertions in `constants.rs` (those are source, not tests).

---

## Step 0 — delete `src/` and `examples/`

`git rm -r src examples`. 11 source files + 1 example removed. v0.1 tree is
preserved at `5324ce5`; nothing was read from it for guidance (per the
brief), and nothing was ported.

## Step 1 — `Cargo.toml` (plan §3)

Verified against `hiss-0.3.2`'s own `Cargo.toml` in the registry
(`~/.cargo/registry/src/index.crates.io-*/hiss-0.3.2/Cargo.toml`), not from
memory:

| Claim | hiss 0.3.2 actual | Match |
|---|---|---|
| `cryptoxide = ">=0.6.0, <0.7"` | `version = ">=0.6.0, <0.7"` | yes |
| `rand_core = "0.10"` | `version = "0.10"` | yes |
| `edition = "2024"` | `edition = "2024"` | yes |
| `rust-version = "1.96"` | `rust-version = "1.96"` | yes |

Both `CLAUDE.md` crypto hard rules therefore hold unchanged.

Also verified the two hiss paths the `constants.rs` const-asserts reach for:

- `hiss::curve::Curve` — `pub trait Curve` at `src/curve/mod.rs:52`, with
  `const PUBLIC_KEY_SIZE: usize;` at :57
- `hiss::curve::p256::P256` — `pub struct P256` at `src/curve/p256/mod.rs:57`,
  `impl Curve for P256` at :59 with `const PUBLIC_KEY_SIZE: usize = 65;` at :61
- `hiss::noise::datagram::MAX_EPOCH_JUMP` — `pub const MAX_EPOCH_JUMP: u64 = 2;`
  at `src/noise/datagram.rs:74`, in a `pub mod datagram`

Neither is behind a cargo feature (hiss's only features are `default` and
`x25519-cryptoxide`, and `curve::p256` is unconditional), so both are
reachable under `default-features = false`. Both are `const`-context
reachable, so plan §4.5 test 5's escape hatch ("if the const assert cannot
be made, drop it") is **not** needed: the const asserts stand *and* the
runtime mirror is written.

`exclude` already carried `"/.slices"` — verified present, not re-added.

Features exactly per plan §3: `default = []`, `test-util = []`,
`sink`, `codec = ["sink", …]`, `tower`.

## Step 2 — `src/constants.rs` (plan §4)

91 constants in table order, 14 private `_MS` companions, and **39
compile-time assertions** in the plan's order. All present; nothing
dropped. Both hiss-reaching assertions compile, so §4.5 test 5's escape
hatch is unused.

Runtime tests written: `timers_are_expressible_in_virtual_time`,
`frame_types_are_distinct`,
`error_codes_are_distinct_and_below_the_application_base`,
`epoch_jump_matches_hiss`. **Not** written: §4.5 test 1
(`named_constants_match_the_consolidated_table`) — the independent
author's.

## Step 3 — `src/error.rs` (plan §5)

Ten types, 41 variants, exactly as §5.1 lists. `#[non_exhaustive]` on
`WriteError` and nowhere else (ruling 61). No `SlitherError`, no `Result`
alias, no `io::Error` conversions, no `Notification`.

Tests written: `connection_lost_is_clone`, `error_types_are_send_and_sync`,
`display_strings_are_non_empty_and_lowercase_initial`,
`embedded_connection_lost_converts`. **Not** written: §5.6 test 1
`taxonomy_is_closed` — the independent author's.

**One correction to the plan, verified empirically.** §5.6 test 4 says
`source()` "chains to" the embedded `ConnectionLost`. It does not:
`#[error(transparent)]` forwards `source()` *through* to the inner error's
own source, so `WriteError::ConnectionLost(TimedOut).source()` is `None`,
not `Some(TimedOut)`. That is the documented behaviour of the attribute and
the right one — a transparent wrapper must not lengthen the chain. The test
asserts what is true (Display forwards unchanged, `source()` is `None`) and
says why in a comment.

## Step 4 — `src/varint.rs` (plan §6)

`pub(crate)`, newtype, the seven functions §6.2 lists. Module carries
`#![allow(dead_code)]`: slice 0 ships the codec but its caller is the frame
layer, so every function but `MAX_VALUE` is reachable only from tests until
then, and `-D warnings` would otherwise fail the lint gate. `#[expect]` was
rejected — under `cfg(test)` the items *are* used, so the expectation would
be unfulfilled and warn in exactly the build that runs the tests.

All seven §6.4 tests written, plus `conversions_are_consistent`.

## Step 5 — `src/shell/{mod,wire.rs}` (plan §7)

Trait transcribed from §16.3 verbatim, native AFIT with
`#[allow(async_fn_in_trait)]` and the comment saying the lint is the design
working. `wire_is_object_unsafe_by_design` written as rustdoc, per §7.4
test 2. Tests: `udp_socket_round_trip`, `a_wire_need_not_be_send`.

## Independent tests: already present and green

`tests/spec_constants.rs` (103 tests) and `tests/spec_errors.rs` (11 tests)
were authored independently from `SPEC.md` and are in the tree. **All 114
pass against this implementation, unmodified** — no constant name and no
constant value had to be reconciled, and no error variant name differed.
That is the transcription check the author swap exists for, and it is
green.

## Step 6 — `src/testutil/mod.rs` (plan §8)

`Network` / `FlakyWire` / `FlakyPolicy` under their attested names, plus
`Spied`, `Tap`, `SendFailure`, `CountingProvider` and `DhCounter`. All 15
§8.7 tests written, including the §9 definition-of-done test verbatim (with
the wall-clock guard §9 asks for).

Determinism, as implemented:

- `Network::seeded(seed)` is the only entropy source. `Network::new()` is
  `seeded(0)`. No OS-entropy path exists.
- One `ChaCha20Rng` per wire, seeded
  `net_seed ^ ordinal.wrapping_mul(0x9E37_79B9_7F4A_7C15)`, with `ordinal`
  an explicit registration counter.
- Every map is a `BTreeMap`/`BTreeSet`, so no hash seed can reach a
  decision path even by accident.
- Fixed draw order per `send_to`, documented on the method: index-based
  drops take **no** draw; otherwise exactly one `f64` for loss, one `f64`
  for duplicate (**taken even when loss already decided the outcome**, so a
  send consumes a fixed number of draws), then one `u64` per delivery for
  jitter (**taken even when `jitter` is zero**, same reason).
- `recv_from` sleeps on `tokio::time::sleep_until` and **pops after the
  sleep**, so a cancelled future loses nothing.

### Three judgement calls inside `testutil`

1. **`SendFailure` error construction.** The plan's struct carries both
   `kind: io::ErrorKind` and `raw_os: i32`, which can disagree, and there
   is no portable `i32` for `ENETUNREACH`. Resolution: a public
   `testutil::ENETUNREACH` constant that is 101 on Linux/Android and 51
   elsewhere; the error is built from `raw_os` when non-zero and from
   `kind` when it is zero. `failing_sends_until` sets both consistently, so
   a test can assert either `err.kind()` (portable) or
   `err.raw_os_error()` (exact). No field was renamed or retyped.
2. **One shared tap log.** §9's test calls `net.tap()` *after* the sends and
   expects 3, so a `Tap` must see history. `Network` keeps one log and
   `tap()` hands out `Rc` clones of it, matching the plan's
   `Tap(/* Rc<RefCell<Vec<Spied>>> */)` sketch. Documented on `Tap`:
   `drain()` empties it for every holder.
3. **`inject` is not a send.** It is not counted in `Network::sends()` and
   is not tapped, because no wire sent it. Documented on the method.

## Step 7 — `src/lib.rs` (plan §2/§9, PLAN.md §9)

Crate docs carry the **five** documentation obligations (S3a, S7, S5, S26,
S30) — five, not four; `PLAN.md` §1 and §9 were reconciled by Q-O4 and the
current text says five in both places, verified. Plus the layer diagram,
the `!Send` rationale, the module tour and the feature table.
`#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`, and the ten error
types re-exported at the crate root.

## Gate-run notes

- **Gate 3 needed one change outside my files.** `cargo clippy -D warnings`
  failed with 7 × `clippy::assertions_on_constants` in
  `tests/spec_constants.rs` (the independently-authored file), all on
  relational fences like `assert!(NO_ERROR < APPLICATION_ERROR_BASE)`. The
  lint's fix (`const { assert!(..) }`) would turn a mis-transcription into
  a build failure and defeat that file's stated "one `#[test]` per constant
  so one wrong value does not mask the rest" design. Added a file-level
  `#![allow(clippy::assertions_on_constants)]` with a comment. **No
  assertion, value or name in that file was changed.**
- **Gate 3 also required `REPLAY_WINDOW % 64 == 0` →
  `REPLAY_WINDOW.is_multiple_of(64)`** (`clippy::manual_is_multiple_of`).
  Verified `is_multiple_of` is `const`-stable on the 1.96 toolchain before
  using it in a const assertion.
- **Gate 6 was verified negatively, not just positively.** Setting
  `IK_MSG1_LEN` to 175 turns the build red with two `E0080` const-eval
  panics (`IK_MSG1_LEN == …` and `INIT_PACKET_LEN == …`) before any test
  runs. Restored immediately. 39 `const _: () = assert!(…)` are in the
  file — the plan's full list, in the plan's order.

---

## The eight gates — final run, all green

```
$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s

$ cargo build --no-default-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s

$ cargo fmt --all --check
(no diff)

$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated .../target/doc/slither/index.html
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated .../target/doc/slither/index.html

$ cargo test
test result: ok. 33 passed; 0 failed   (lib)
test result: ok. 103 passed; 0 failed  (tests/spec_constants.rs, independent)
test result: ok. 11 passed; 0 failed   (tests/spec_errors.rs, independent)
test result: ok. 0 passed; 0 failed    (doc-tests)

$ cargo test --all-features
(identical: 33 / 103 / 11 / 0)

# Gate 6 — wire pins. No golden vectors until slice 1, so the pins are the
# 39 compile-time assertions plus the independent transcription test.
# Verified NEGATIVELY: IK_MSG1_LEN 174 -> 175 turns the build red with
# E0080 on both `IK_MSG1_LEN == ...` and `INIT_PACKET_LEN == ...`.

$ cargo +1.96 check --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

Definition-of-done test:

```
$ cargo test --all-features a_byte_crosses_two_flaky_wires_under_injected_loss
test testutil::tests::a_byte_crosses_two_flaky_wires_under_injected_loss ... ok
```

## Not committed

`git rm -r src examples` is **staged, not committed** — no commit was
asked for. The tree is: staged deletions of the 11 v0.1 sources and the one
example, plus untracked `src/{constants,error,varint}.rs`, `src/shell/`,
`src/testutil/`, `tests/` and this log, plus modified `Cargo.toml` and
`src/lib.rs`.

`Cargo.lock` is regenerated but stays untracked, per `CLAUDE.md`.

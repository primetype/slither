# Lens B — test adequacy review, slice 0 (commit `73757c3`)

Question asked of every test: **what would have to break for this test to
fail?** Method: mutate the code under test, run the relevant test target,
record what (if anything) went red, then revert.

Baseline: `cargo test --all-features` — 147 tests, all green (confirmed
before starting).

Every mutation below is reverted before the next one starts. Final state is
verified against `73757c3` with `git status --porcelain` and `git diff
--stat`, both required to be empty.

## Mutation log

| # | File | Mutation | Command run | Result | Caught by |
|---|------|----------|-------------|--------|-----------|
| 1 | `src/varint.rs` `encoded_len` | `self.0 < (1 << 6)` → `<=` (off-by-one on the 1-byte/2-byte boundary; 64 now wrongly reports length 1) | `cargo test --lib varint` | **CAUGHT** | `boundaries_round_trip`, `exhaustive_round_trip_near_boundaries` |
| 2 | `src/varint.rs` `decode` | `buf.len() < n` → `<=` (rejects an exactly-sized buffer as truncated, e.g. `[0x25]` alone now decodes to `None`) | `cargo test --lib varint` | **CAUGHT** | `boundaries_round_trip`, `decode_accepts_every_non_minimal_encoding_of_a_small_value`, `encode_to_respects_a_short_buffer`, `exhaustive_round_trip_near_boundaries`, `rfc_9000_appendix_a1_vectors` |
| 3 | `src/testutil/mod.rs` `FlakyWire::send_to` | `deliveries` match rewritten so `drop_first`/`drop_at`/`loss` are computed but never actually reduce the delivery count to 0 (a policy configured for total loss now delivers everything) | `cargo test --all-features --lib testutil` | **CAUGHT** | `a_byte_crosses_two_flaky_wires_under_injected_loss` (the slice-0 DoD test itself), `drop_at_is_exact`. Notably `seeded_runs_are_identical` / `different_seeds_diverge` did *not* catch this — they only check trace equality/inequality, not that loss actually happened — but the DoD test and `drop_at_is_exact` are sufficient on their own. |
| 4 | `src/testutil/mod.rs` `Network::endpoint` | `wire_seed` XORed with `SystemTime::now()` nanos, so the per-wire RNG stream is no longer a pure function of the network seed | `cargo test --all-features --lib testutil::tests::seeded_runs_are_identical` (×3 runs) | **CAUGHT**, every run | `seeded_runs_are_identical` — exactly ruling 60's determinism contract as a test, and it fired every time |
| 5 | `src/testutil/mod.rs` `deliver()` | `inner.endpoints.get_mut(&dst)` → `&src` — every datagram is routed back into the *sender's own* inbox instead of the destination's (a routing/addressing bug) | `cargo test --all-features --lib testutil` | **CAUGHT, but as an infinite hang, not a red test.** `cargo test --lib testutil` never returned in 120 s; isolating the DoD test alone (`a_byte_crosses_two_flaky_wires_under_injected_loss`) also hung indefinitely and had to be killed manually. Root cause: several tests call `b.recv_from(&mut buf).await.expect("recv")` with **no surrounding `tokio::time::timeout`** on the call that is expected to succeed (the DoD test itself, `perfect_policy_delivers_everything_in_order`, `duplication_delivers_two_identical_copies`'s second recv, `oversize_datagram_is_truncated_like_a_socket`, others). With nothing ever delivered to `b`, `recv_from`'s `self.notify.notified().await` blocks forever with no timer to advance, so tokio's paused-clock auto-advance never kicks in (there is no pending timer, just a hung `Notify` wait) and `cargo test` hangs rather than failing. This *would* eventually be caught by an external CI job timeout, but locally and in a fast-feedback loop it presents as a stall, not a red X — a materially worse signal than every other mutation tried. See "Coverage gaps" below. |
| 6 | `src/testutil/mod.rs` `FlakyWire::send_to` | Injected-send-failure branch gated with `if false && ...` — a configured `send_failure` is computed but never returned as an `Err` | `cargo test --all-features --lib testutil::tests::send_failure_is_an_err_and_then_heals` | **CAUGHT** | `send_failure_is_an_err_and_then_heals` (panicked at `"the send must fail"`) |
| 7 | `src/testutil/mod.rs` `CountingProvider::dh` | Deleted the `self.dhs.set(self.dhs.get() + 1);` line — `dh()` is called but the counter never increments | `cargo test --all-features --lib testutil::tests::dh_counter_counts_only_dh` | **CAUGHT** | `dh_counter_counts_only_dh` (asserted `1` after first `dh()` call, got `0`) |
| 8 | `src/constants.rs` `MAX_PLAINTEXT` | A **derived** constant (`1170` → `1171`), `MAX_DATAGRAM` left alone | `cargo build --all-features` | **CAUGHT at the build**, never reaches a test run | 2 `const _: () = assert!(…)` compile-time assertions (`MAX_PLAINTEXT == MAX_DATAGRAM - DATA_HEADER_LEN - AEAD_TAG_LEN`, `MAX_DATAGRAM_PAYLOAD == MAX_PLAINTEXT - 1`) |
| 9 | `src/constants.rs` `MAX_ACK_RANGES` | A **base** constant with no downstream `const` assertion anywhere in the file (`64` → `63`) | `cargo build` then `cargo test --test spec_constants` | Build succeeds; **CAUGHT** at test time | `max_ack_ranges` in `spec_constants.rs` (the only fence covering this constant) |
| 10 | `src/constants.rs` — 5 constants at once | `MAX_DATAGRAM` (1200→1201), `MAX_PLAINTEXT` (1170→1171), `MAX_DATAGRAM_PAYLOAD` (1169→1170), `INITIAL_WINDOW` (12000→12010), `MINIMUM_WINDOW` (2400→2402) — chosen so **every** `const` assertion in `constants.rs` that relates them stays internally satisfied (a "consistently wrong" drift the build-time fence structurally cannot see) | `cargo build --all-features` then `cargo test --test spec_constants` | Build **succeeds** (all 39 assertions pass); **CAUGHT** at test time regardless | `max_datagram`, `max_plaintext`, `max_datagram_payload`, `initial_window`, `minimum_window` — all 5 independent literal tests failed. This is the strongest evidence in this review that `spec_constants.rs`'s literal restatements are a *second, independent* fence and not merely decorative alongside the build asserts: a mutation engineered specifically to be invisible to the compile-time checks is still caught, because each literal test compares against a value transcribed from `SPEC.md`, not against another crate constant. |
| 11 | `src/error.rs` `ConnectError` | Added a tenth, unused variant `MutationProbe` to an exhaustive (non-`#[non_exhaustive]`) enum | `cargo build --all-features` then `cargo test --test spec_errors` | Lib **builds fine** and `cargo test --lib` (the crate's own unit tests) still **passes** — nothing inside the crate itself exhaustively matches these enums. `tests/spec_errors.rs` **fails to compile**: `E0004: non-exhaustive patterns: ConnectError::MutationProbe not covered` | `match_connect_error` in `spec_errors.rs`, as a compile error, exactly as its module doc claims |
| 12 | `src/error.rs` `AcceptError` | Removed the `EndpointDropped` variant (the opposite direction: a variant deleted rather than added) | `cargo build --all-features` then `cargo test --test spec_errors` | Lib builds fine; `spec_errors.rs` **fails to compile**: `E0599: no variant ... EndpointDropped found` | `match_accept_error` |
| 13 | `src/error.rs` `WriteError` (the **one** `#[non_exhaustive]` type) | Added an eleventh variant `MutationProbe` (standing in for adding `Stopped` early, which ruling 61 explicitly forbids until the STOP_SENDING round is ratified) | `cargo build --all-features`, `cargo test --all-features` (full suite, 147 tests) | **UNDETECTED.** Lib builds clean. `cargo test --all-features` — all 147 tests, including all 11 in `spec_errors.rs` and `write_error_is_exhaustively_matched` specifically — **pass**, no red anywhere. | **Nothing.** Root cause: `write_error_is_exhaustively_matched` is `let _: fn(WriteError) = match_write_error;` — it takes the function as a value and never *calls* it. `match_write_error`'s `_ => panic!(...)` arm (the "load-bearing... stays loud by panicking" arm the module doc at `spec_errors.rs:186-201` explicitly claims makes an actually-new variant "still caught (at test-run time...)") is therefore dead code: it can only fire if some test constructs a `WriteError::MutationProbe` and passes it through `match_write_error`, and none does. **This directly contradicts the file's own doc comment**, which is exactly the kind of gap this review exists to find: the fence's own documentation asserts a guarantee the code does not provide. Anyone adding `WriteError::Stopped` today, before the STOP_SENDING round is ratified — the exact thing ruling 61 says `#[non_exhaustive]` is *not* a licence to do — would sail through every gate in `CLAUDE.md`'s release-gate table undetected. |
| 14 | `src/shell/wire.rs` `impl Wire for UdpSocket::send_to` | Body deleted, returns `Ok(buf.len())` without calling the real socket's `send_to` at all — a delete-body-return-default mutation on the only kernel-touching code in slice 0 | `cargo build --all-features` then `cargo test --lib shell::wire::tests::udp_socket_round_trip` | **CAUGHT, but as an infinite hang**, same class as mutation #5. `assert_eq!(sent, 4)` is fooled (the stub returns the right length), but `Wire::recv_from(&b, ...)` then blocks on a real kernel socket for a UDP datagram that was never sent, with **no timeout** around the call. Killed manually after 120 s. | Would eventually surface as a CI timeout, not a clean assertion failure — `udp_socket_round_trip` is the crate's one non-paused-clock test and has no bounding timeout, unlike its `testutil` counterparts. |
| 15 | `src/testutil/mod.rs` `FlakyWire::recv_from` | Reordered so the queued datagram is popped out of the inbox **before** `tokio::time::sleep_until(deliver_at).await` rather than after — breaks §8.2's "pop after sleep" cancel-safety rule the doc comments call out as "THE line that makes §16.10 work" | `cargo build --all-features` then `cargo test --lib testutil::tests::recv_from_is_cancel_safe` | **CAUGHT, but as an infinite hang.** The test's first `recv_from`, wrapped in a 10 ms `tokio::time::timeout`, is cancelled while sleeping — but the datagram was already popped into a local that the cancelled future drops, so it is lost for good. The test's *second* `recv_from` (`.await.expect("recv")`, no timeout) then blocks forever on a `Notify` that will never fire again, since no further send is coming. Had to be killed manually. | This is the **third** independent mutation (with #5 and #14) where breaking a "something should arrive" invariant in `testutil` produces a silent hang rather than a clean red test, because the success-path `recv_from` call in the relevant test has no `tokio::time::timeout` around it. |
| 16 | `src/testutil/mod.rs` `FlakyWire::send_to` | Restructured so the `loss`/`duplicate` `f64` draws happen **unconditionally**, even for a `drop_at`/`drop_first`-dropped index — violates the documented draw-order contract's step 1 ("`drop_at` / `drop_first` — index-based, **no draw at all**") | `cargo build --all-features` then `cargo test --all-features` (full 147-test suite) | **UNDETECTED.** All 15 `testutil` tests pass, and so does the entire 147-test suite. | **Nothing.** No test constructs a `FlakyPolicy` that combines `drop_at`/`drop_first` with non-zero `loss`/`duplicate`/`jitter` in the same run, so nothing observes that the RNG stream position for later sends has shifted. `drop_at_is_exact` uses `drop_at` alone (loss/duplicate both `0.0`, so extra draws that compare `< 0.0` are always `false` and change nothing observable); the probabilistic-loss tests (`seeded_runs_are_identical`, `different_seeds_diverge`, the DoD test) never mix in `drop_at`/`drop_first`. This is a real, specific, confirmed coverage gap in the test suite (not just a hypothetical) — see "Coverage gaps" below. |

**Summary: 16 mutations tried. 11 caught cleanly (1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12). 3 caught only as an infinite hang, not a clean test failure (5, 14, 15). 2 fully undetected by every gate in `CLAUDE.md`'s table (13, 16).**

## Answers to the specific questions

### 1. `tests/spec_constants.rs` — is it a real fence?

Yes, and mutation #10 is the decisive evidence. I picked five constants
(`MAX_DATAGRAM`, `MAX_PLAINTEXT`, `MAX_DATAGRAM_PAYLOAD`, `INITIAL_WINDOW`,
`MINIMUM_WINDOW`) and changed all five together, by amounts specifically
chosen so every one of the 39 build-time `const` assertions in
`constants.rs` that relates them stays satisfied — a mutation engineered to
be invisible to the compile-time fence. The build succeeded. `spec_constants.rs`
still failed on all five, because each has its own `#[test]` asserting
against a literal transcribed from `SPEC.md`, not against another crate
constant. That is a genuinely independent check, not a restatement of the
same relationship the build already enforces.

The "derived relationship" tests at the foot of the file (e.g.
`max_plaintext_is_datagram_minus_header_minus_tag`) are weaker than they
look: they import every value `use slither::constants::*` and recompute one
from the others, which is the *same* relationship the corresponding `const`
assertion in `constants.rs` already checks, just re-expressed in a second
file. They are not vacuous — a regression that removed or weakened the
build-time assertion would still be caught here — but they add a second
layer over the *same* invariant, not an independent one. The **literal**
`#[test] fn constant_name()` functions are where the independence lives,
and mutation #9 (a base constant, `MAX_ACK_RANGES`, that participates in
*no* `const` assertion at all) confirms they are load-bearing on their own:
nothing else in the crate would have caught that one.

Coverage check: every one of the 91 `pub const` items in `constants.rs` is
referenced at least once in `spec_constants.rs` (verified by diffing the
two symbol sets) — no constant is silently unfenced.

### 2. `tests/spec_errors.rs` — the exhaustiveness claim

Tested directly, both directions (mutations #11 and #12): adding a tenth
variant to `ConnectError`, and separately removing `AcceptError::EndpointDropped`,
each **fail the compile** of `spec_errors.rs` with the expected `rustc`
error (`E0004` / `E0599`), while the crate's own `cargo build` and
`cargo test --lib` are unaffected either way — confirming the fence lives
entirely in the separate test crate, exactly as the module doc claims.

But mutation #13 shows the claim is **false for the one type that most
needs it**. `WriteError` is `#[non_exhaustive]`, so its fence function
(`match_write_error`) needs a wildcard arm to compile from outside the
crate; the file's own doc comment (lines 174–203) asserts that arm "stays
loud by panicking rather than silently doing nothing, so an actually-new
variant is still caught (at test-run time...)". That is not true as
written: `write_error_is_exhaustively_matched` only takes
`match_write_error` as a function pointer (`let _: fn(WriteError) = ...`)
and never calls it, so the panic arm is unreachable dead code in every test
run. Adding an eleventh variant to `WriteError` today — precisely what
ruling 61 says `#[non_exhaustive]` is *not* a licence to do before the
STOP_SENDING round is ratified — passes every one of the eight release
gates in `CLAUDE.md` silently. This is the most important finding in this
review.

### 3. The 39 compile-time assertions in `constants.rs` — load-bearing or self-satisfying?

Both, depending on which constant. Confirmed load-bearing for a genuinely
*derived* constant: mutating `MAX_PLAINTEXT` alone (mutation #8) fails the
**build** via two of the assertions, before any test even runs — exactly
the design intent stated in the module doc ("a wrong value fails the
build, not a test run"). But `MAX_DATAGRAM` turned out to be more tightly
cross-linked than a first read suggests — it feeds `MAX_PLAINTEXT`'s
derivation *and* `INITIAL_WINDOW`/`MINIMUM_WINDOW`'s RFC-9002 derivation,
so a naive "change one, keep the rest" mutation doesn't stay
build-green; it takes changing five constants together (mutation #10) to
find a combination the 39 assertions cannot see. That combination exists,
and is caught only one layer up, by `spec_constants.rs`'s literal tests —
which is the correct division of labour (`CLAUDE.md`'s "wire pins" gate
runs `cargo test`, not just `cargo build`, so both layers are part of the
same release gate in practice) but is worth naming plainly: the
`const` assertions alone are not sufficient, only necessary.

For a *base* constant with no downstream assertion (`MAX_ACK_RANGES`,
mutation #9), the build is fully self-satisfying — nothing checks it at
compile time, and the only fence is the literal test in `spec_constants.rs`.

### 4. Does the slice-0 DoD test prove a byte crossed **under loss**?

Yes. Mutation #3 is the direct proof: I made `FlakyPolicy` never actually
reduce the delivery count to zero (loss and `drop_at`/`drop_first` are
computed but ignored), and
`a_byte_crosses_two_flaky_wires_under_injected_loss` failed on its own
"the two dropped datagrams never arrive" assertion — the exact assertion
that requires two of the three sends to be genuinely absent. Tracing the
test's own logic confirms this isn't an accident of that one mutation: with
`FlakyPolicy::drop_at([0, 1])` configured, the test asserts `net.sends() ==
3` (all three left the wire) but expects only **one** delivery and then a
1 s `timeout` that must return `Err`. A lossless wire would deliver a
second and third copy that the final `timeout(...).is_err()` assertion
would catch immediately. The test is not decorative — it requires drops to
really happen, not just to be configured.

### 5. Coverage gaps

Two are concrete and confirmed by mutation, not speculative:

- **The undetected `WriteError` variant addition (#13).** The exhaustiveness
  fence for the one type ratified to grow a variant does not actually run.
  Any later slice that adds `WriteError::Stopped` early — before the
  STOP_SENDING round — gets no signal from any of the eight release gates.
  A fix is straightforward (call `match_write_error` on a constructed
  sample of each *documented* variant inside a `#[should_panic]`-free
  assertion, or restructure so the wildcard path is exercised), but as
  shipped, the protection this file's own comments promise does not exist.

- **The `drop_at`/`drop_first` draw-order contract is untested in
  combination with probabilistic loss (#16).** The doc on `FlakyWire::send_to`
  states the draw order is "part of the determinism contract rather than
  an implementation detail" and specifically calls out that index-based
  drops take "no draw at all". No test exercises a `FlakyPolicy` that
  combines `drop_at`/`drop_first` with non-zero `loss`, `duplicate`, or
  `jitter` in the same run, so a regression that makes index-dropped sends
  silently consume RNG draws — shifting every later send's outcome by one
  draw — passes the entire suite. Later slices that build "the first N
  packets die, then normal jittery loss resumes" fixtures (a very likely
  pattern for handshake-retry tests, given Appendix B's "the first two
  msg1s die" fixture already uses bare `drop_first`/`drop_at`) would be
  building on an interaction that has never actually been exercised.

Three more are softer, all a variant of the same shape (#5, #14, #15): a
break that removes an expected delivery in `testutil` — a routing bug, a
stubbed-out real-socket `send_to`, a cancel-safety violation in
`recv_from` — is caught, but as an **unbounded hang** rather than a red
test, because several success-path `recv_from` calls across the test suite
(the DoD test itself, `perfect_policy_delivers_everything_in_order`,
`udp_socket_round_trip`, others) have no `tokio::time::timeout` wrapper.
`cargo test` does not fail fast on these; it stalls, and every later slice
that adds tests to the same binary would stall behind it too, with no
indication of which test is stuck short of `--nocapture` bisection. This
is not a false negative — CI would eventually time out and mark the job
red — but it is a materially worse failure mode than every other class of
bug this review found, and it costs real time to diagnose. Wrapping the
success-path `recv_from` calls that currently lack one in a generous
`tokio::time::timeout` (as most of the other tests already do) would turn
these into fast, clear failures.

## A note on the working tree

A second agent (Lens A, fidelity review) was editing `CLAUDE.md`,
`Cargo.toml` and `PLAN.md` concurrently with this review (ruling-count
updates, 59 → 63). Those changes are not mine, were not reverted by me, and
are outside this review's scope. Every mutation in the table above was
made only to files under `src/` and `tests/`, each was verified reverted
individually with `git diff --stat -- <file>` immediately after undoing it,
and a final `cargo test --all-features` (147/147) confirmed the crate
behaves identically to `73757c3` after every mutation was undone.

## Verdict

**ADEQUATE, with two confirmed gaps worth fixing before later slices build
on top of them.** Every wire constant and every closed-taxonomy error
variant (except one) is genuinely fenced, independently of the
implementation, by both a compile-time and a test-time check — verified by
mutation, not by reading. The slice-0 definition-of-done test really does
require loss to occur, determinism is enforced and checked from two
directions (a broken-loss mutation and a non-deterministic-seed mutation
both failed loudly), and the DH-count ladder later slices depend on is a
real counter, not a fiction. Against that: the `WriteError` exhaustiveness
fence is dead code for the one variant addition it exists to catch, and the
`drop_at`/probabilistic-loss interaction has no test at all. Both are small,
specific, and fixable; neither is evidence of a broader pattern of
decorative testing across the slice.

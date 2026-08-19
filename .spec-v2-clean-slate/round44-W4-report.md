# Round 44 — W4 report: examples/echo.rs

## Base commit
721167a — Round 43: the SECV5 pins land — the one-shot keepalive, the spent-packet separator, the 272 pin

## Task
Create examples/echo.rs per round44-D-fix-plan.md §6, using round44-A-hurried-dev.md
section "(i) The working code" as source template.

## Deviations from §6

**One behavioural deviation, found by running the program, not by reading
the spec.** §6's addition list asks for the dialler calling `recv_message`
for the echo reply, "so both verbs appear on both sides" — it does not
mention `acked()`. A's proven template calls `.acked()` after `send_message`
and before `close()` on its one send path, and my first draft kept that
shape symmetrically on the *answerer's* send-the-echo-back path (the second
`send_message` in the file, matching the first for consistency).

That first draft compiled, formatted and clippy-passed, but panicked at
`cargo run --example echo` on the third gate:

```
thread 'main' panicked at examples/echo.rs:72:32:
acked: PeerClosed { code: 0, reason: [100, 111, 110, 101] }
```

The race: the dialler receives the echo via `recv_message()` and, having
what it came for, immediately calls `close()` — nothing in the example makes
it wait for anything else. The answerer, meanwhile, is still awaiting its
own `acked()` after sending that same echo. The dialler's close reaches the
answerer first, and `acked()` resolves `Err(PeerClosed)` instead of `Ok(())`,
which the `expect()` turns into a panic. This is not flaky — it reproduced
on every run of the first draft.

Fix: dropped the answerer's `.acked()` call. It was never load-bearing here
— the dialler's `recv_message()` on the reply is itself the proof the echo
was delivered, which is a stronger guarantee than `acked()` gives on its own
send path. Removing it also brought the file from 90 to 89 lines. Re-ran
`cargo run --example echo` six times after the fix (three during the
verification pass below, three more afterward); all six exited 0 with
identical output shape.

Two cosmetic deviations, both to hit the 90-line budget without breaking
`cargo fmt`:
- socket bind variables are named `dial_sock`/`ans_sock` rather than
  `dial_socket`/`answer_socket` (A's names) — rustfmt's default chain-width
  heuristic (60, independent of the 100-column line limit) forces
  `.bind(..).await.expect(..)` onto three lines regardless of overall line
  length once the receiver name is long enough to push the chain over that
  threshold, so shorter names were the only way to keep the bind calls from
  costing an extra line each.
- `answerer.public_static().clone()` (A's exact line) became
  `*answerer.public_static()`: `clippy::clone_on_copy` — the P-256 public
  key type is `Copy` — fires under `-D warnings` even though A's version
  built and ran fine (A never ran clippy on it).

Everything else follows §6 as written: `//!` header with the `block_on`
warning; second-person present-tense comments, one line per idea; numbered
steps `1.`–`7.` matching §4.3's quickstart exactly (suite declaration,
`block_on`, two identities, the out-of-band key print, `Wire`/endpoint
setup, the `accept()`-is-a-loop comment, `connect()`'s 0-DH note);
`send_message`/`recv_message` on both sides; only `slither`, `hiss`,
`rand_chacha`, `getrandom`, `tokio` as deps (all already in `Cargo.toml`,
no `[[example]]` stanza needed — confirmed via `cargo metadata` showing
`examples/audit_udp.rs` and `examples/bench_vs_tcp.rs` auto-discovered the
same way).

## Verification

**1. `cargo build --example echo`**
```
   Compiling slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.62s
```

**2. `cargo run --example echo`** (representative run, exit 0; six total runs
after the fix, all exit 0 with the same shape)
```
answerer public key: [4, 96, 64, 204, 64, 33, 3, 255, 14, 66, 84, 151, 86, 116, 12, 22, 120, 106, 10, 9, 101, 122, 117, 104, 24, 122, 157, 23, 54, 48, 129, 249, 158, 153, 157, 172, 72, 35, 118, 27, 199, 28, 223, 145, 72, 110, 29, 150, 229, 102, 199, 12, 101, 105, 113, 184, 62, 19, 69, 89, 156, 83, 103, 32, 92]
answerer received: "hello"
dialler received: "hello"
round trip complete
exit: 0
```

**3. `cargo fmt --check -- examples/echo.rs`**
```
(no output)
fmt exit: 0
```
(First pass failed on the pre-fix draft's manually-collapsed bind lines;
ran `cargo fmt -- examples/echo.rs` to apply rustfmt's chain-break
formatting, then re-checked clean.)

**4. `cargo clippy --example echo -- -D warnings`**
```
    Checking slither v0.2.0 (/Users/nicolasdiprima/work/primetype/slither)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.32s
```
(First pass failed with `clippy::clone_on_copy` on
`answerer.public_static().clone()`; fixed to `*answerer.public_static()`,
re-ran clean.)

## Final line count

89 lines (`wc -l examples/echo.rs`), under the ≤90 budget.

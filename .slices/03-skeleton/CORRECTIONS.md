# Corrections to `.slices/03-skeleton/PLAN.md`

Found by the orchestrator during dispatch. The plan is otherwise sound and
is not being rewritten — these are recorded so a later reader does not
follow them, and so slice 3b's dispatch inherits the fix.

## 1. §2.1's `tests/spec_frames.rs` and `tests/spec_close.rs` cannot exist

`src/lib.rs:127` declares `pub(crate) mod core;`. Every type slice 3a
builds — `core::Connection`, `ConnOutput`, `ConnEvent`, the frame codec,
the replay window — is therefore **unreachable from an integration test
in `tests/`**, which compiles as an external crate against slither's
*public* API only.

The three files already in `tests/` (`spec_constants.rs`,
`spec_errors.rs`, `spec_packet.rs`) test `pub mod constants`,
`pub mod error` and `pub mod packet`. They work because those modules are
public. `core` is not, and should not become public to satisfy a test.

**Correction applied at dispatch.** TEST-A owns exactly one file,
`src/core/connection/tests.rs` — a new in-crate test module. Its `mod`
declaration is added by the orchestrator at integration, so that IMPL-A
never has to create or reference a file it is forbidden to own, and so
IMPL-A's build stays green for its whole run rather than failing on a
missing module. This is a deliberate improvement on the slice-2a
arrangement, where the implementer declared the module and could not run
`cargo test` until the test author landed.

**Slice 3b is unaffected.** Its `tests/story_lifecycle.rs`,
`tests/story_dial.rs` and `tests/spec_shell.rs` test the *shell*, which
is `pub mod shell` — genuinely public, genuinely reachable from `tests/`,
and the right place for a story test written the way a consumer would
write it.

*Why it was missed:* the plan reasoned from Appendix B's obligations,
which are written against the finished protocol and say nothing about
module visibility. Working rule 8's shape once more — a stated
construction (the test file layout) with an unstated scope (what a
`tests/` file can reach).

## 2. §7.2 of `.slices/02-handshake/PLAN.md` is wrong and stays wrong

Recorded here rather than fixed, because it is slice 2's artefact and the
error is already documented in `.slices/02-handshake/OPEN-QUESTIONS.md`
D4. It pins the §17.1 guard at `authenticate()`; §17.1 puts it at
`read_identity()` and the implementation follows the spec. Slice 3's
plan was briefed not to inherit it and did not.

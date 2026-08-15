//! In-crate tests for `slither::core` — slice 2a, "the sans-io endpoint
//! core".
//!
//! **Authorship (CLAUDE.md working rule 6, `.slices/02-handshake/PLAN.md`
//! §9.1).** This file is written by an author who has **not** read
//! `src/core/`'s implementation: the tests come from `SPEC.md` and from the
//! plan's declared API, and the implementation is written against them. One
//! author writing both can make both wrong in a mutually consistent way and
//! leave CI green — which is exactly what happened once already, and is why
//! slice 1 caught a coverage gap that 187 passing tests and a clean
//! fidelity review both missed.
//!
//! **This file is a stub.** The implementing agent for slice 2a created it
//! empty so that `#[cfg(test)] mod tests;` compiles, and added no `#[test]`
//! anywhere under `src/core/`. **Slice 2a's core behaviour is therefore not
//! yet test-covered.** The test author's file replaces this one.
//!
//! # What the tests here look like, and what they must not
//!
//! **Every test in this module is a plain `#[test]`.** No `#[tokio::test]`,
//! no `tokio::time::pause()`, no `LocalSet`, no `FlakyWire`. That falls
//! straight out of §16.4: `now: Instant` is an argument and the cores never
//! read a clock, so time is `let t0 = Instant::now();` and `t0 +
//! Duration::from_secs(5)`. The paused clock is right for the shell and
//! unnecessary here.
//!
//! Two rules the assertions have to respect:
//!
//! * **DH costs are cumulative, never per-call.** §6.1's dagger note gives
//!   a pre-read or frozen entry its cached result at 0 *incremental* DH
//!   while leaving the cumulative table unchanged. "`read_identity` costs
//!   1" is false for those entries; "cumulative after `read_identity` is 1"
//!   is true for all of them.
//! * **Output order within one drain is normative** (§16.4), so asserting
//!   `outs[0]` before `outs[1]` tests a rule rather than an implementation
//!   detail.
//!
//! Slice 1's precedent for where in-crate tests live is `src/packet/tests.rs`;
//! `tests/spec_constants.rs`, `tests/spec_errors.rs`, `tests/spec_packet.rs`
//! and `src/packet/tests.rs` are **not** touched by this slice.

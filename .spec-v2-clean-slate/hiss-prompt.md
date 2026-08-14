# hiss: additions for slither wire v2 (one minor release)

Context: slither (`../slither`, same author) has a review-hardened wire-v2 spec
draft whose Appendix A gates on two hiss additions plus two doc/cosmetic fixes.
Before starting, read `../slither/SPEC-v2-DRAFT.md` — the "Appendix A — hiss
dependencies" section near the end — for the full rationale; it cites exact
hiss `file:line` evidence for each item (verified against the `v0.3.1` tag on
2026/08/13; line numbers may have drifted slightly). Implement all four items
on one branch. Follow this repo's CLAUDE.md conventions and release gates
throughout (fmt, clippy `-D warnings`, docs `-D warnings`, tests, cargo deny,
MSRV). All existing tests — the Cacophony vector suite especially — must stay
green.

## 1. Split read of handshake message 1 (the load-bearing item)

Motivation (from slither): slither's staged accept must recover msg1's claimed
static after the `es` DH, then **suspend** — the result is an app-held object
parked across event-loop turns, possibly for seconds (human-in-the-loop accept
decisions) — and only later pay the `ss` DH. The existing `Verify` read style
hands the claimed static to a closure but decides synchronously inside one
read call, so it cannot suspend without re-paying `es` later; slither's DoS
cost ladder (1 DH to inspect, 2 cumulative to authenticate) forbids that.

Required semantics (names are suggestions — `read_message_1_intro` / `Mid` /
`complete()` — pick better ones if you have them):

1. **Phase 1** consumes the msg1 buffer, processes tokens up to and including
   decrypting + validating the claimed static under the `es`-derived key
   (exactly one DH: `es`), and yields a mid-handshake state that (a) exposes
   the claimed static by value, and (b) owns the unprocessed message tail as a
   fixed-size array — no borrow into the input buffer, no re-supply of bytes
   at phase 2.
2. **Phase 2** (`complete(self, …)`, consuming) performs `ss`, decrypts the
   msg1 payload, and verifies the tag. On success it returns the decrypted
   payload and the same responder state the one-shot read would have produced
   (ready to write msg2; transcript byte-identical to the one-shot path). On
   failure it returns an error and yields **neither** payload nor state.
3. Dropping the mid state abandons the handshake; follow the existing
   scrub-on-drop conventions for whatever key material it holds.
4. Scope: required for msg1 shapes like IK's `e, es, s, ss [N]`. Generalise
   across patterns only if the codegen stays clean; emitting the split pair
   only when msg1's token sequence ends `…, s, ss` is acceptable.

Where: this is codegen in `hiss-macros` — the read surface today is exactly
three styles (`Plain`/`Lookup`/`Verify`, `hiss-macros/src/codegen.rs:930-943`)
with at most two generated methods per message (`codegen.rs:958-984`). Keep
the existing styles intact and unchanged. Prior ballpark estimate: ~300–450
lines of codegen.

Tests: (a) intro+complete equals the one-shot read — same claimed static,
same decrypted payload, byte-identical msg2 and transcript hash afterwards;
(b) tampered tail: `complete()` fails, yields nothing, cannot be retried;
(c) dropping the mid state scrubs per convention; (d) Cacophony IK vectors
pass unchanged.

## 2. `DatagramSend::next_counter()`

A `&self` accessor returning the counter the next **successful**
`encrypt_next` will seal under (the current cipher-state `n`). No mutation.
Document the guarantees: the returned value equals what the next successful
`encrypt_next` returns; failed seals leave it unchanged; at `u64::MAX` the
next seal fails `NonceOverflow` (the accessor still returns `u64::MAX`).

Motivation: slither's cleartext data header contains the counter and is the
AEAD associated data, so the header must be constructed **before** sealing;
today the counter is only learnable from `encrypt_next`'s return value, which
forces downstream mirror-and-assert bookkeeping of hiss-owned state.

Tests: `next_counter()` equals the counter returned by the following
`encrypt_next` — across many seals, across an epoch-ratchet boundary, and
unchanged after a failed (oversize) seal.

## 3. Doc-only: the canonical-encoding stability promise

Do NOT change the `Curve::PublicKey` bound — no API change here. The shipped
`PublicKey` types already implement `AsRef<[u8]>` (P-256 stores the
normalised 65-byte uncompressed encoding — `src/curve/p256/mod.rs:145-150`),
and downstream consumers can require it with their own where-clause. What
they cannot self-provide is the **stability promise**, so add it to the
docs (on the `Curve` trait and/or each shipped curve's `PublicKey`):

> Where a curve's `PublicKey` exposes `AsRef<[u8]>`, those octets are the
> curve's **canonical public-key encoding** (P-256: 65-byte uncompressed
> SEC1; X25519: 32-byte u-coordinate) and MUST NOT change for a given curve
> across releases. Downstream protocols key MACs over these octets and
> compare them as identity tie-breaks, so the encoding is wire-relevant:
> changing it is a wire-breaking change for consumers even when the Rust
> API is unchanged.

Non-breaking, doc-only — no semver implication. Optionally add a per-curve
test pinning the encoding (length; P-256 leading `0x04` uncompressed tag) so
a future refactor trips a test rather than silently re-keying a downstream
MAC.

## 4. One-line wart: zero `h` on drop

`SymmetricState::drop`'s comment promises to zero `h` "for defence in depth"
but the code does not (`src/noise/symmetric_state.rs:202-208`; `ck` and the
cipher keys do self-scrub). Make the code match the comment.

## Release

One changelog entry per item. Run the full release-gate suite. Do not
publish and do not tag; stop after the gates are green and report (a) what
you built per item, and (b) any place where the implemented API shape
diverged from the suggestions above, so the slither spec's Appendix A can be
updated to match. Nothing here is semver-breaking (items 1–2 are additive,
3–4 are doc/cosmetic), so a 0.3.x minor is the expected shape.

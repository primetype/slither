# A.1 split msg1 read — slither's side of the contract

Written 2026/08/14, ahead of reviewing the hiss session's implementation
plan. This is the alignment reference: what slither's ratified spec
*requires*, what it merely *prefers*, and the checklist the plan is
reviewed against. hiss owns the API design; slither owns these
constraints.

## 1. Hard requirements (ratified; a plan that misses one is a no)

**H1 — The mid-state must SUSPEND, not decide.** The claimed static is
surfaced to the *application* (`read_identity()` → `Claimed`), which may
take seconds to answer (human-in-the-loop accept is a motivating use
case). The mid-state is parked in slither's endpoint core across
event-loop turns, keyed by `IntroId`, and only later completed. This is
why the shipped `Verify` closure cannot serve: it decides synchronously
inside the read.

**H2 — The DH ladder is exactly 1 then 2 cumulative.** Phase 1 pays
exactly one DH (`es`); phase 2 pays exactly one more (`ss`). No design
where the accepted path re-pays `es` is admissible — slither's DoS
bounds (§6.1, §6.9, and Appendix B's DH-cost pins) are priced on it, and
the spec states there is **no fallback path**.

**H3 — The mid-state is self-contained and owned.** No borrow of the
input datagram buffer, no lifetime parameter, and phase 2 must NOT
require re-supplying any message bytes. slither drops the receive buffer
the moment `handle_datagram` returns.

**H4 — Phase 2 consumes.** A failed `complete()` yields neither payload
nor a usable next state, and cannot be retried. (Consuming `self` gives
this for free.)

**H5 — Transcript equivalence.** intro+complete must produce byte-identical
results to the one-shot read: same claimed static, same decrypted
payload, and a byte-identical msg2 and transcript hash afterwards. The
split is an API affordance, not a protocol variant — nothing may appear
on the wire.

**H6 — The claimed static is available by value at phase 1.** It is
handed to the application and stored in slither's staged chain.
`Curve::PublicKey: Clone` (0.3.1) suffices; no new bound is needed.

**H7 — Scrub on drop.** Dropping the mid-state abandons the handshake
and scrubs its key material per hiss's existing conventions. slither
drops mid-states routinely: `reject()`, `INTRO_TTL` expiry (15 s), and
queue eviction all discard them.

## 2. Preferences (hiss's call; state the choice, no ruling needed)

- Names. `read_message_1_intro` / `Mid` / `complete()` were *suggestions*
  in the prompt, not requirements. A better hiss-idiomatic naming is
  welcome.
- Generality. slither needs only msg1 shapes ending `…, s, ss` (IK's
  `e, es, s, ss [N]`). Generalising is welcome if the codegen stays
  clean. A natural generalisation, if it helps: **split at the `s`
  token** — phase 1 = "up to and including the decrypted `s`", phase 2 =
  "everything after it" — which needs no special-casing of what follows
  and covers `s, ss` as one instance.
- Whether the split pair is emitted alongside or instead of the `_with`
  variant on qualifying messages. slither uses neither `Plain` nor
  `Verify` on msg1, but asks that the three existing styles stay intact
  and unchanged for hiss's other consumers.
- Whether phase 2 is inherent or trait-based.

## 3. Concrete numbers for the reference case

slither's msg1 is IK `e, es, s, ss` + a 12-byte payload. Wire sizes
(P-256 reference suite; §2.3's algebra):

| Piece | Bytes |
|---|---|
| `e` (uncompressed point) | 65 |
| `s` (encrypted static + tag) | 65 + 16 = 81 |
| payload + tag | 12 + 16 = 28 |
| **`IK_MSG1_LEN`** | **174** |

Because `ss` consumes no wire bytes, the split point sits at
65 + 81 = **146**, and the carried tail is exactly the payload plus its
tag — **28 bytes** (X25519: split at 80, same 28-byte tail). In general
for `…, s, ss [N]` the tail is `N + 16`, known at codegen time from the
existing size constants, so a fixed-size array is the natural carrier
(the prompt's "option (a)").

Mid-state size is therefore dominated by the symmetric state and the
provider, not the tail — consistent with the spec's ≈ 0.5–1 KB estimate
(§6.3, §17.5), which is what slither's queue bound (1024 entries) is
sized against. A materially larger mid-state would need a spec revisit,
so the plan should state the expected size.

## 4. Review checklist for the incoming plan

- [ ] H1–H7 each satisfied, explicitly
- [ ] Exact DH count per phase stated (must be 1 and 1)
- [ ] Tail carrier named, sized, and lifetime-free
- [ ] Mid-state size estimate given
- [ ] Failure semantics on `complete()` (consume, no partial application)
- [ ] Drop/scrub behaviour named
- [ ] Equivalence test described (claimed static, payload, msg2 bytes,
      transcript hash)
- [ ] Tamper test and drop-scrub test described
- [ ] Cacophony suite unaffected
- [ ] The three existing read styles unchanged
- [ ] Codegen scope stated (which token sequences qualify)
- [ ] Anything hiss considers wrong/over-specified in the prompt, raised

## 5. Notes that changed since the prompt was written

- slither's msg1 payload is now **12 bytes** (timestamp only) and
  **msg2 carries no payload at all** — the continuation flags were
  deleted by the ratchet-only ruling (2026/08/14). The prompt's
  "msg2 `[N]` payload supported as-is" non-item is therefore moot for
  slither; it does not affect A.1.
- A.3 is doc-only (encoding-stability promise); the permanent mechanism
  is slither's own `where C::PublicKey: AsRef<[u8]>` clause, and `Ord`
  is **not** required (the §6.7 tie-break compares equal-length
  `as_ref()` octets).
- After A.1 lands, Appendix A is reconciled against the shipped API.

---

## 6. Review of hiss's plan (2026/08/14)

**Verdict: conforms on all seven hard requirements.** Checklist:

| Req | Status | Evidence in the plan |
|---|---|---|
| H1 suspend | ✓ | `read_message_1_intro` → owned mid-state → later `complete()` |
| H2 ladder 1+1 | ✓ | intro emits recv_e→es→recv_s; complete emits ss→recv_tail |
| H3 owned tail | ✓ | `tail: [u8; MSG1_INTRO_TAIL]` copied in; no lifetime; complete takes no bytes |
| H4 consume | ✓ | `complete(self)`; retry impossible at compile time |
| H5 equivalence | ✓ | identical support-call sequence; no new runtime crypto code |
| H6 static by value | ✓ | returned by value AND retained via `claimed_static(&self)` |
| H7 scrub | ✓ | inherited (SymmetricState ck+h, provider key Drop); tail is public ciphertext |

**Better than asked:** (i) the claimed static is both returned by value and
retained in the mid-state, which serves freeze-on-carry re-inspection
without slither storing it separately; (ii) they will replay the Cacophony
IK corpus *through the split path*, giving a third-party oracle rather than
self-consistency only.

**Verified independently:** the struct-level `CryptoKeyProvider` bound with
`DhProvider` on the impls is hiss's existing convention for generated
states (`codegen.rs:677-708`), not an error. The mid-state is deliberately
not `Clone` (a clone could replay `complete()`) — correct.

**DH accounting, pinned mutually:** intro = 1 (`es`), complete = 1 (`ss`),
`write_message_2` = 2 (`ee`, `se`) ⇒ **4 total for an accept**, exactly
§6.1's ladder and Appendix B's pins.

**Their three pushbacks — all factually correct, accepted:**
1. The drop-scrub test is not honestly runtime-testable (observing freed
   memory is UB in safe Rust). Appendix A's test list says "scrub inherited
   structurally" instead of implying a runtime check.
2. "Cannot be retried" is a compile-time property (`complete(self)`), not a
   runtime latch. Appendix A rephrased.
3. Scope widens costlessly to IKpsk0-shaped msg1 (`psk, e, es, s, ss`).
   Appendix A says "IK"; widen to "msg1 shapes ending `…, s, ss`".

**Consequent Appendix A edits (slither side, no wire impact):** mid-state
type name `Mid` → `IKResponderMsg1Intro`; the "at most two generated
methods per message (codegen.rs:958-984)" citation goes stale (qualifying
msg1 states will carry three read methods); the three items above.

**Open minor suggestion to hiss:** `#[must_use]` on the mid-state type, so
an accidental `let _ = …intro()` is caught. Slither's deliberate
drop-to-reject is unaffected (an explicit drop still rejects).

## 7. Sign-off (2026/08/14)

**Maintainer approved the plan as written; hiss is implementing phase 1
and phase 2.** All three hiss pushbacks accepted. The `#[must_use]`
suggestion was passed along as optional.

**A.1's `[MAINTAINER]` flag stays OPEN deliberately** — Appendix A is
reconciled against the shipped API in one pass when the hiss work merges,
then ratified. Pending Appendix A edits, queued for that pass:
- test list item (c): runtime drop-scrub check → "scrub inherited
  structurally from `SymmetricState` and provider-key `Drop` impls";
- unretryability stated as compile-time (`complete(self)`), not a latch;
- scope: "IK" → "msg1 shapes ending `…, s, ss`" (includes IKpsk0);
- mid-state type name `Mid` → `IKResponderMsg1Intro`;
- drop the stale "at most two generated methods per message
  (codegen.rs:958-984)" citation — qualifying msg1 states carry three;
- state the real mid-state `size_of` (requested from hiss) against
  §6.3/§17.5's ≈0.5–1 KB estimate and the 1024-entry queue bound;
- A.3 → doc-only, `Ord` dropped, where-clause permanent;
- delete the msg2-`[N]`-payload non-item (msg2 now carries no payload).

## 8. Delivery verified (2026/08/14) — hiss `slither-wire-v2` 4b63402

Independently verified in the hiss tree, not taken on report:

- **API shape matches the approved plan exactly**:
  `read_message_1_intro(&msg1) -> (claimed_static, IKResponderMsg1Intro<CP>)`,
  then `mid.complete() -> (payload, IKResponderMsg2<CP>)`.
- **DH ladder pinned by a counting provider** (`tests/noise_macro_shapes.rs`):
  `dhs == 1` after intro ("intro pays exactly the one `es` DH"), `dhs == 2`
  cumulative after complete. Exactly §6.1's ladder — 1 to inspect, 2 to
  authenticate. `staged_reject_by_drop_costs_exactly_one_dh` pins the
  drop-to-reject path at 1 DH: slither's `reject()`, `INTRO_TTL` expiry
  and queue eviction all cost one DH and never `ss`.
- **Tamper semantics pinned**: intro succeeds and reveals the claimed
  static; `complete()` returns `DecryptionFailed`, consuming the state.
- **Equivalence pinned**: byte-identical msg2 and session id vs the
  one-shot read; Cacophony IK vectors replay byte-identical through the
  split path.
- **Tests run locally and pass**: 5/5 staged tests; the full Cacophony
  suite 328 passed, 0 failed.

**Mid-state size, measured** (reference suite, `EphemeralOnly<StdRng>`):
784 B on P-256 (616/648 B on X25519), of which ~320 B is the provider
itself; `MSG1_INTRO_TAIL` = 28 as derived. The delta over the pre-read
state slither already holds is **32 B**, not state-plus-message. 1024
parked mid-states ≈ **0.77 MiB** — inside §6.3/§17.5's ≈0.5–1 KB
estimate, which therefore stands unamended (state the real figure).

**Divergences from the approved plan, all benign for slither:**
1. One-way patterns (X, Xpsk0) qualify too; there `complete()` yields the
   `Transport` directly. No slither impact.
2. IKpsk1 (trailing psk) deliberately excluded and documented — its
   `complete()` would need the PSK re-supplied; the `_with` lookup style
   serves that shape. No slither impact.
3. `#[must_use]` on the mid-state — our optional suggestion, adopted.
4. Tail const `MSG1_INTRO_TAIL` on the pattern type, WireSize-derived.
5. Generated docs gained a staged walkthrough; the downstream gate gained
   a fifth arm compiling it.

**Consequence: Appendix A.1's gate is satisfied.** Reconciliation (the §7
edit list) runs once the in-flight security-fix surgery releases the
spec file. Release: hiss 0.3.2, additive, **cut deferred to the
maintainer** — nothing is published.

## 9. hiss 0.3.2 published (2026/08/14)

Tags `v0.3.2` / `hiss-macros-v0.3.1`, on crates.io, all hiss gates green
(bench and Interop included). Carries the whole wire-v2 round: staged
msg1 read, `DatagramSend::next_counter()`, zero-`h`-on-drop, per-curve
canonical-encoding pins.

**Verified against slither locally:** re-resolved hiss 0.3.1 → 0.3.2
(hiss-macros 0.3.0 → 0.3.1); `cargo build --all-features --all-targets`
clean; `cargo test` 76 passed / 0 failed plus 3 doctests — the existing
(old-wire) implementation is unaffected, as expected for an additive
round. cryptoxide (`>=0.6.0, <0.7`) and `rand_core` (0.10) bounds are
unchanged in 0.3.2, so CLAUDE.md's exact-range pinning rule needs no
edit.

**Cargo.toml floor deliberately NOT bumped yet.** `version = "0.3.1"`
already admits 0.3.2 under caret semantics, and the shipped code calls
none of the new APIs. The floor rises to `0.3.2` in the same change that
first calls `read_message_1_intro`/`next_counter()` — i.e. with the
wire-v1 implementation — so the constraint keeps stating what the code
actually needs.

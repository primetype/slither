# Ruling 248's eight API/doc decisions (round 39) — status at 94dab20

Task: round41-material.md item 13 ("Ruling 248's eight API/doc decisions
(round 39) — untouched.", `round41-material.md:72`).

## Source

Ruling 248 header: `.spec-v2-clean-slate/rulings.md:7164` — "### 248 —
open for the maintainer, from this round". The eight-item list is at
`rulings.md:7166-7185`. Cross-referenced (with fuller citations) from
`.spec-v2-clean-slate/round39-worklist.md` Part B, "needs a maintainer
decision. `rulings.md` §248" (`round39-worklist.md:81-96`, items B1-B8,
which map 1:1 onto ruling 248's items 1-8 and add citations into
`review-api-opus-r39.md`).

**Base commit checked:** `94dab20`. Commits since ruling 248's base
(`6448e6f`) were enumerated with `git log --oneline 6448e6f..94dab20` —
44 commits, all round 40 (rulings 249-255) plus round 41's rulings
256-257. None of their titles touch API surface, `ConnEvent`, `BiStream`,
`Config`, CI workflows, or STORIES.md. Verified by reading rulings
249-257's headers (`rulings.md:7215,7325,7407,7469,7523,7596,7655,7723,7791`)
— none is about the eight items below.

---

## Item 1 — `ConnEvent` has 14 variants; §16.4 lists 13, §16.2 lists 10; `SendCreditAvailable` is in neither

> "§16.4's `ConnEvent` block lists 13 variants and §16.2 re-enumerates 10;
> the code has 14. The missing one is `SendCreditAvailable`, which
> **ruling 150 explicitly authorised** and which appears nowhere in
> `SPEC.md` — `grep SendCreditAvailable SPEC.md` returns nothing. Working
> rule 8's defect class, in the section that rule was written about.
> Needs a spec amendment, not a code change."
> — `rulings.md:7166-7171`

**Status: STILL OPEN.** `grep -n SendCreditAvailable SPEC.md` still
returns zero hits at `94dab20`. The variant is live in code:
`src/core/connection/mod.rs:3486` (`ConnEvent::SendCreditAvailable`,
inside the `pub(crate) enum ConnEvent` at `mod.rs:3427`), emitted at
`mod.rs:1702`, consumed at `src/shell/driver.rs:654`, and discussed in
rustdoc at `mod.rs:3572` and `src/shell/shared.rs:301` — but SPEC.md's
§16.4 variant list and §16.2's re-enumeration are unamended.

**Decision the maintainer must take:** whether/how to bring SPEC.md's two
`ConnEvent` listings in line with the 14-variant reality.
- Option A — amend §16.4's list to add `SendCreditAvailable` (13→14) and
  fix §16.2's re-enumeration (10→11, or however many it should track).
- Option B — same, but also decide whether §16.2's list is meant to be a
  strict subset of §16.4's (if so, state the subset rule explicitly, since
  right now neither count matches the other nor the code).

**Cost:** doc-only (SPEC.md text edit + a `rulings.md` amendment entry
per the project's ratification process). No code change implied — the
code is already right per ruling 150. Cheapest of the eight; text-only,
single already-known variant name to insert.

---

## Item 2 — `BiStream::join` has two different signatures in the spec

> "`BiStream::join` has two different signatures in the spec. §16.2 gives
> `Result<Self, (SendStream, RecvStream)>` (ruling 120); §16.11 gives
> `-> Self`. The code follows §16.2. Reported, not resolved — working
> rule 3."
> — `rulings.md:7172-7174`

**Status: STILL OPEN.** Both spec passages are unchanged:
- §16.2, `SPEC.md:5232`: `pub fn join(send: SendStream, recv: RecvStream) -> Result<Self, (SendStream, RecvStream)>;` (comment: "Err = not the same stream").
- §16.11, `SPEC.md:6342`: `pub fn join(send: SendStream, recv: RecvStream) -> Self;`

Code (`src/shell/stream.rs:975-983`) implements the §16.2 `Result` form,
with a `#[expect(clippy::result_large_err, ...)]` explicitly citing
"ruling 120 fixes this signature".

**Decision the maintainer must take:** which of the two spec sections is
wrong and needs amendment — §16.2 or §16.11 — since the code cannot match
both.
- Option A — amend §16.11 to match §16.2's fallible `Result` form (matches
  shipped code and ruling 120; zero code change).
- Option B — amend §16.2/ruling 120 to the infallible `-> Self` form and
  change the code to match (would be a breaking API change, semver-relevant
  even pre-1.0, and reopens ruling 120's rationale for the fallible design,
  which the doc comment on `stream.rs:967-968` treats as deliberate:
  "The `Err` variant is deliberately large — it *is* the two handles").

**Cost:** Option A is doc-only. Option B is a small code change (API
signature change, panics instead of Result) plus reopening ruling 120 —
the crate is 0.2.x/unpublished so semver is not a hard blocker, but it
reverses a previously-ratified decision (120) with reasoning already on
record against it in the code comment itself.

---

## Item 3 — `ConnectionId`/`IntroId`: re-exported, no public signature, no public constructor/accessor

> "`ConnectionId` and `IntroId` are re-exported at the crate root, appear
> in no public signature, and have no public constructor or accessor.
> `lib.rs` also says "three identifiers" and re-exports five. Dead
> surface about to be frozen under semver."
> — `rulings.md:7175-7178`

**Status: STILL OPEN.** `src/lib.rs:302-307`:
```
// The three identifiers §16.4's surface names that a consumer must be able
// to spell. ...
pub use crate::core::{ConnectionId, Dir, IntroId, StreamId, Timestamp};
```
The comment still says "three identifiers" while the `pub use` still
names five types. `ConnectionId::from_raw` (`src/core/mod.rs:96`) and
`IntroId::from_raw` (`src/core/endpoint/staged.rs:57`) are both
`pub(crate)`. `grep`ing all `pub fn`/`pub(crate) fn` signatures across
`src/` turns up no public function taking or returning `ConnectionId` or
`IntroId` — every call site (`endpoint/mod.rs:467`, `endpoint/tables.rs:61,66`,
`endpoint/intro_queue.rs:306,319,324,330`, `endpoint/staged.rs:121,131,783`,
`shell/staged.rs:68`) is `pub(crate)`.

**Decision the maintainer must take:** what to do with two re-exported
types that are unreachable from any public API surface.
- Option A — un-export both (drop them from `lib.rs:307`'s `pub use`,
  fix the "three identifiers" comment to name the actual three: `Dir`,
  `StreamId`, `Timestamp`). Cheapest, and matches what the code already
  supports (nothing public names them).
- Option B — give them a real public purpose: a public accessor
  somewhere (e.g. `Connecting`/`Connection` could expose their
  `ConnectionId`, or `staged`/`Incoming` could expose `IntroId`) plus a
  public constructor if a consumer is meant to construct one. This is a
  design question (is there a story that needs a consumer to hold or
  compare these ids?) — none was found in STORIES.md during this check.

**Cost:** Option A is doc/API-surface-shrink only (removing dead public
re-exports) — small code change, arguably not even semver-relevant this
early. Option B is a real API addition needing new rustdoc and likely new
tests — the more expensive path, and nothing in the code half-does it
today.

---

## Item 4 — no public route to a second `Connection` handle

> "No public route to a second `Connection` handle. `clone_handle` is
> `pub(crate)` and the crate uses it for its own owned `Service`;
> `Rc<Connection>` is the answer, is used in `tests/story_lifecycle.rs:850`,
> and is mentioned in no rustdoc."
> — `rulings.md:7179-7183`

**Status: STILL OPEN.** `clone_handle` (`src/shell/connection.rs:124`) is
still `pub(crate)`, and is now additionally gated
`#[cfg(feature = "tower")]` (its doc comment explains this is the ruling
246 fix for a dead-code warning outside that feature — see item 7). No
rustdoc on `Connection` (`src/shell/connection.rs:39-63`) mentions
wrapping the handle in an application-held `Rc<Connection>` as the
supported way to get a second handle; the doc only describes the two drop
rules. `tests/story_lifecycle.rs:850`'s comment ("The watcher is a
separate spawned task holding an `Rc<Connection>`") is still the only
place this pattern is written down, and it is a test comment, not
rustdoc.

**Decision the maintainer must take:** whether app-side `Rc::new(connection)`
is the sanctioned pattern for a second handle, and if so, whether that
needs to be *said* publicly.
- Option A — doc-only: add a rustdoc paragraph on `Connection` (or in the
  crate-level docs) stating that wrapping the owned `Connection` in an
  `Rc` (app-side, no crate API needed) is the supported multi-handle
  pattern, since `Connection` requires no special crate cooperation to be
  shared this way.
- Option B — promote `clone_handle` to `pub` (or add a dedicated public
  `Connection::clone_handle`/`Connection::handle()` method) so the crate
  itself hands out the second handle rather than relying on the consumer
  to `Rc`-wrap the owned value.
- Option C — do nothing; treat the test comment as sufficient prior art
  and decline to formalize.

**Cost:** Option A is doc-only, cheapest, and matches what the code
already permits (no crate change needed — plain `Rc::new` around the
existing owned `Connection` already works, this is purely a documentation
gap). Option B is a small API surface addition/visibility change.

---

## Item 5 — 11 public types lack `Debug`

> "11 public types lack `Debug` — all eight `compat::stream` adapters
> plus `Connect`, `OpenBi`, `OpenBiOwned` (and seven in `testutil`). Rust
> API guideline C-DEBUG."
> — `rulings.md:7184-7185`

**Status: STILL OPEN.** Checked all 11 named types at `94dab20`; none
derive or manually implement `Debug`:
- `src/compat/stream.rs`: `Messages` (360), `Datagrams` (384),
  `IncomingBi` (404), `IncomingUni` (427), `Notifications` (447),
  `Incoming` (481), `MessageSink` (510), `DatagramSink` (578) — no
  `#[derive(...Debug...)]` above any of the eight, and `grep -n 'impl.*Debug'`
  over the file finds none for these types.
- `src/compat/tower.rs`: `Connect` (106), `OpenBi` (195), `OpenBiOwned`
  (265) — same, no `Debug`.
The "seven in `testutil`" were not individually re-verified (out of
scope for the headline check) but nothing in the round-40/41 commit log
touches `src/testutil`.

**Decision the maintainer must take:** whether to add `Debug` to these 11
(+7) types now, in one pass.
- Option A — derive `Debug` on all of them (most have simple
  lifetime/generic-bounded fields; likely mechanical for most).
- Option B — manual `impl Debug` where a field genuinely shouldn't be
  printed (e.g. anything holding a raw key material reference — worth
  checking case by case, though these particular 11 look like
  stream/future adapters rather than key holders).
- Option C — leave as-is and record the guideline deviation explicitly
  (not recommended per C-DEBUG but is a legitimate maintainer call).

**Cost:** small code change — mostly mechanical `#[derive(Debug)]``
additions across ~18 types, unless any hold non-`Debug` fields (e.g. an
`Rc<dyn Trait>` or a closure) that need a manual impl or
`finish_non_exhaustive()`-style writer, in which case it's a few
one-off manual impls (still small, not a design decision). Semver
concern is minimal (adding a trait impl is additive).

---

## Item 6 — STORIES.md S18 is stale: `ConnEvent::AddressMoved` is `pub(crate)`

> "STORIES.md S18 is stale: it has the application observing
> `ConnEvent::AddressMoved`, which is `pub(crate)`; ruling 46 routed it to
> `Notification`. The code is right and the story text was not swept."
> — `rulings.md:7186-7188`

**Status: STILL OPEN.** `STORIES.md:301` still reads: "The application
observes `ConnEvent::AddressMoved` and `remote_address()` reflects the
new address." `ConnEvent` is `pub(crate)` (`src/core/connection/mod.rs:3427`),
and `AddressMoved` as an application-visible type is
`Notification::AddressMoved` (`src/shell/shared.rs:621`, surfaced via
`Connection::notified()`/`notifications()` per
`src/shell/connection.rs:255`'s doc example). This is purely a
documentation sweep gap — the code is correct.

**Decision the maintainer must take:** none, really — this is a one-line
text fix with no design content (it's the same defect class as item 4/6
"the story text was not swept").
- Option A (only real option) — edit `STORIES.md:301` to say
  `Notification::AddressMoved` instead of `ConnEvent::AddressMoved`.

**Cost:** doc-only, one line, no ambiguity. Cheapest fix in the set along
with item 1's spec-text-only nature — this one doesn't even need a
`rulings.md` ratification entry arguably, since it's correcting a
transcription error rather than deciding anything, though the project's
own working rule 4 would say to record it.

---

## Item 7 — a feature-matrix CI job

> "A feature-matrix CI job, per 246. Eight combinations currently pass;
> nothing keeps them passing."
> — `rulings.md:7189-7190`

**Status: STILL OPEN.** `.github/workflows/test.yml`'s matrix
(`test.yml:20-32`) has exactly two feature points: `label: "default"`
(`flags: ""`) and `label: "all features"` (`flags: "--all-features"`),
crossed with two OSes. `Cargo.toml:48-73` defines 4 independent
opt-in features (`test-util`, `sink`, `codec` implies `sink`, `tower`;
`default = []`), which per ruling 246's framing yields a combination
lattice CI does not fully cover — only the two extreme points (all-off,
all-on) are built. `check.yml`'s `clippy-doc` job is also only
`--all-features --all-targets` plus default-feature doc build — same two
points. No new job or matrix expansion was added between `6448e6f` and
`94dab20`.

**Decision the maintainer must take:** whether to add a feature-power-set
(or a curated subset) CI job, and how exhaustive.
- Option A — full power-set (2^4 = 16, or "eight combinations" per the
  ruling's count if `codec` implying `sink` collapses some) via
  `cargo hack --feature-powerset` or an explicit matrix list.
- Option B — a curated subset covering each feature in isolation plus a
  couple of pairs known to interact (e.g. `tower` alone, since that's
  where ruling 246's `clone_handle` dead-code bug actually lived), rather
  than the full power-set — cheaper CI minutes, less exhaustive.
- Option C — decline; rely on `--all-features` and manual spot-checks
  (status quo).

**Cost:** small code change (CI YAML only, no crate code), but scales
with option: Option A adds meaningfully to CI runtime (more matrix legs);
Option B is close to free. Nothing in the repo half-implements this yet
— `cargo-hack` is not a dependency and no `Justfile`/script target for it
was found.

---

## Item 8 — window auto-tuning

> "Window auto-tuning, per 247(a). Today's constants cap a single stream
> at `256 KiB / RTT` — about 2.5 MiB/s at 100 ms — with no way for a
> consumer to raise them. `Config` exposes no flow-control knob."
> — `rulings.md:7191-7193`

**Status: STILL OPEN.** `src/constants.rs:270`:
`pub const INITIAL_MAX_STREAM_DATA: u64 = 262_144;` (256 KiB), unchanged,
with three `const _: () = assert!(...)` invariants at `constants.rs:609-612`
still tying it to `MESSAGE_RECV_MAX` and `INITIAL_MAX_DATA` — this is a
ratified constant, explicitly not to be changed without a ruling (per
CLAUDE.md's wire-pins rule and ruling 247(a)'s own framing: "no constant
was touched"). `Config` (`src/config.rs:58-63`) has exactly four fields —
`intro_queue_cap`, `intro_max_per_source`, `epoch_size`, `clock` — and
four `with_*` builder methods (`config.rs:123,162,183,190`); none touches
flow control or stream/connection window sizing.

**Decision the maintainer must take:** whether slither wants
configurable/auto-tuned flow-control windows at all — this is a protocol
design decision, not a bug fix, and the ruling is explicit that "no
constant was touched" (the ratified value stands until a ruling changes
it).
- Option A — add a `Config` knob to override `INITIAL_MAX_STREAM_DATA`/
  `INITIAL_MAX_DATA` per-endpoint (static, consumer-chosen at
  construction) — closest in shape to the existing `with_intro_queue_cap`
  pattern, no protocol change, just makes an already-ratified constant
  configurable rather than hardcoded.
- Option B — implement dynamic auto-tuning (e.g. BBR/Cubic-style receive
  window growth based on measured RTT/throughput) — a substantially
  larger protocol-and-implementation change, needs its own spec section
  and rulings, well beyond a `Config` field.
- Option C — decline; document the 2.5 MiB/s@100ms ceiling as a known
  limitation for this wire version.

**Cost:** Option A is a small-to-medium code change (new `Config` field +
builder + threading it into the flow-control init path) with no wire
change (the *value* sent on the wire can already vary per §7's flow
control frames — this would just change what value this endpoint
advertises). Option B is a genuine protocol-design project, closer to "a
new slice" than a small fix, and would need spec sections plus new
rulings. Nothing in the code currently half-implements either — the
constant is still a `const`, not a field.

---

## SUMMARY TABLE

| # | One-line | Status | Decision needed? | Cost |
|---|---|---|---|---|
| 1 | `ConnEvent` has 14 variants; §16.4/§16.2 undercount, `SendCreditAvailable` unspecced | STILL OPEN | Y (which spec text to amend) | doc-only |
| 2 | `BiStream::join`: §16.2 says `Result`, §16.11 says `-> Self`; code follows §16.2 | STILL OPEN | Y (which section is wrong) | doc-only (§16.11 fix) or API-change (reverse ruling 120) |
| 3 | `ConnectionId`/`IntroId` re-exported, unreachable from any public signature, no ctor/accessor | STILL OPEN | Y (un-export vs. give them a purpose) | small code (un-export) or API addition (give purpose) |
| 4 | No public route to a second `Connection` handle; `clone_handle` is `pub(crate)`+`tower`-gated | STILL OPEN | Y (document `Rc`-wrap vs. add public API) | doc-only or small API-visibility change |
| 5 | 11 public types (8 `compat::stream` + `Connect`/`OpenBi`/`OpenBiOwned`, +7 `testutil`) lack `Debug` | STILL OPEN | Y (add now vs. defer) | small code (mostly `#[derive]`) |
| 6 | STORIES.md S18 says `ConnEvent::AddressMoved`; should say `Notification::AddressMoved` | STILL OPEN | N (pure sweep fix) | doc-only |
| 7 | No feature-matrix CI job; only 2 of the feature lattice's points are built | STILL OPEN | Y (full power-set vs. curated subset vs. decline) | CI-YAML only, cost scales with scope |
| 8 | `Config` has no flow-control/window knob; 256 KiB/RTT cap is hardcoded | STILL OPEN | Y (static knob vs. real auto-tuning vs. decline) | small code (static knob) to protocol-scale (auto-tuning) |

All eight items are **STILL OPEN** — none was touched, resolved, or
mooted by rulings 249-257 or any commit between `6448e6f` and `94dab20`.

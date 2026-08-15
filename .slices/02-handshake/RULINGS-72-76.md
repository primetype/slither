# Rulings 72-76 — application notes

Commit at start: ab6fc4b

## Status
- [ ] Read CLAUDE.md working rules 8/9/10
- [ ] Read rulings 72-76 (tail rulings.md)
- [ ] Read amended SPEC passages
- [ ] R72 — IntroError::Local / ConnectError::Local
- [ ] R73 — orphan aging from pin release
- [ ] R74/75 — verify, doc-comment citations
- [ ] R76 — equal-deadline order
- [ ] Gates

## Ruling texts (verbatim excerpts)

## Spec passages

## Code survey

## Changes made

## Test results

## Findings / declined

### R72 — SPEC §18.1 (lines 5047-5081)
- `ConnectError::{AlreadyConnected, TimedOut, Local}`; `Local` = *our own* `Identity::open()` failed
  (locked/biometrics-gated enclave, hardware fault, momentarily-unavailable provider).
- `IntroError::{Expired, Internal, Malformed, Local, EndpointDropped}`;
  `Malformed`: peer's bytes at fault, **chain discarded**, 1 DH spent, definitive.
  `Local`: *our own* provider failed, **chain left parked**, retry can still succeed.

### R74/75 — SPEC §6.1 (lines 1058-1079)
- `read_identity()` idempotent: 2nd call on Claimed/Proven returns revealed static, 0 DH, opens no provider (r74).
- `authenticate()` advances a still-parked chain, drives the skipped `es`, lands exactly 2 DH cumulative (r75).
- Structurally unreadable msg1 discards chain (Malformed); `Local` leaves it parked.

### R73 — SPEC §17.1 (lines 4851-4862)
- Orphan `TS_GUARD_ORPHAN_TTL` runs from the instant its **last pin is released**, not last successful admission.
- Mitigation (iii) untouched: LRU **recency** remains admission-only; only the **aging clock** starts at release.

### R76 — SPEC §16.5 (lines 4564-4584)
- Equal-deadline list is **exhaustive** and normative. Principle: *a terminal outcome precedes a
  routine one, and state removal precedes emission.*
- Endpoint order: (1) give-up, (2) intro expiry, (3) guard-orphan aging, (4) retransmit.

## Finding A — `Local` cannot carry a source (declining half of the brief)
Brief: "carrying the provider's error as a source **where the taxonomy allows it**".
The taxonomy does **not** allow it:
- `ConnectError`/`IntroError` derive `Clone, PartialEq, Eq`; `src/error.rs`'s
  `error_types_are_send_and_sync` asserts `Send + Sync + 'static` for both.
- `Identity::Error` is bounded only `::core::error::Error + 'static` — no `Send`,
  no `Sync`, no `Clone` (src/identity.rs:103). Deliberately: the enclave provider
  is `!Send` by design (module docs, S21).
- A `Box<dyn Error + Send + Sync>` payload would need bounds `Identity` does not
  have; an `Arc<dyn Error>` payload still breaks `Send + Sync` and `Eq`.
- §18.1 (SPEC 5047-5081) names `Local` with **no payload**, while it does spell out
  payloads elsewhere (`PeerClosed { code, reason }`, `Reset(u64)`).
=> Implemented as a **unit variant**. Widening `Identity::Error`'s bounds is a
   trait/spec change and would need a ruling.

## Finding B — `tests/spec_errors.rs` will stop compiling (by design)
Its `match_connect_error` / `match_intro_error` fences have no `_` arm (deliberate).
Adding `Local` to either enum breaks the fence at compile time — that is exactly
what the file's own header says should happen. It is a forbidden path (working
rule 6 / brief). Not edited. Blocks `build --all-targets` and `test`.

## Finding C — ruling 73 collides with §16.4's ratified signatures (BLOCKING the brief's method)
The brief: "`unpin()` therefore needs `now: Instant` threaded in".
§16.4's ratified API block (SPEC.md:4368-4386) gives **no `now`** to:
  - `fn handle_connection_event(&mut self, id: ConnectionId, ev: ToEndpoint);`
  - `fn reject(&mut self, id: IntroId);`
  - `fn read_identity(&mut self, id: IntroId) -> Result<PublicKey, IntroError>;`
All three reach `TimestampGuard::unpin`:
  - `handle_connection_event(Retired)` -> `drop_pending` / `statics.remove_by_connection` -> `unpin`
    — this is **exactly** the path ruling 73's security argument is about ("the moment it retires").
  - `reject(id)` -> `discard_chain` -> `release_chain_guard_state` -> `unpin` (Claimed/Proven chains hold a pin).
  - `read_identity`'s Malformed discard: a `Parked` entry carries no `guard_pin`, so no unpin — not a gap.
Threading `now` would require amending two ratified signatures => needs a ruling. Spec wins (brief rule 2).

### [SUPERSEDED by Finding C, revised — kept because the reasoning still stands, the conclusion does not]
### First attempt: a **forward** stamp, never a backward watermark
Rejected: caching a `last_now` watermark and using it for the clockless verbs. A connection that dies of
`DEAD_TIMEOUT` was last heard from 25 s ago, so the watermark is 25 s stale and
`stale + TS_GUARD_ORPHAN_TTL (15 s) < now` — the entry dies at the next sweep with **zero orphan window**.
That is ruling 73's defect reproduced verbatim, one level down.

Built: `orphaned_at: Option<Instant>`; `unpin()` (still clockless) drops `pins` and marks the guard
`has_unstamped`. The **next clocked call** stamps every unpinned, unstamped entry with that `now`
(`TimestampGuard::observe`). An unstamped orphan has **no** `age_deadline` — it cannot be swept and
contributes no deadline — so the error is always in the safe direction (a window slightly too long,
never too short). `age_orphans(now)` stamps before it sweeps, which is what makes ruling 76's
give-up(1) -> orphan-aging(3) grant a *fresh* window, as the rulings text predicts.
`has_unstamped` is a plain `bool`, so the common path is one branch, not an O(n) scan on the flood path.

## Finding C, revised — the forward stamp was WRONG; the tests proved it
First build ran `cargo test`: 2 failures, both in the independent suite.
`the_endpoint_deadline_covers_guard_orphan_aging` asserts that the drain **immediately after**
`handle_connection_event(Retired)` announces `Some(t + TS_GUARD_ORPHAN_TTL)`. A purely forward
stamp cannot: the release instant is unknown until the *next* clocked call, so the deadline is
`None`. **The test is right and my first design was wrong** — §16.5 lists orphan aging as one of
the endpoint's three deadline families, and a family that goes momentarily silent is not that.

### Final design: a provisional stamp with a first-observation floor
- `Endpoint::last_now` — the last instant the core was **told** (set in `new`, advanced by every
  clocked verb). Not a clock read.
- `unpin(key, last_now)` stamps `orphaned_at = Some(last_now)` and marks the stamp **provisional**.
  The deadline is therefore announceable at once (test above passes).
- `observe(now)` — at the **first** observation after the release, if the provisional stamp has
  *already* expired (`orphaned_at + TTL <= now`) it is re-stamped to `now`; either way the stamp
  becomes final. So: **an orphan always gets a full TTL from the first instant the core can see it
  is an orphan**, which is precisely ruling 73's "no orphan window at all" failure, closed.
- Without that floor, a backward watermark reproduces ruling 73's defect: a connection dying of
  `DEAD_TIMEOUT` leaves `last_now` 25 s stale, and `stale + 15 s` is already past.

## Finding D — ruling 73 opens a refresh channel mitigation (iii) was written to close
`src/core/tests.rs::an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl` **fails, legitimately.**
Traced empirically (temporary `eprintln!` in `unpin`, since reverted) — three unpin-to-zero events:
  1. `t`                (the `Retired`)
  2. `t + TTL - 1ns`    (**the below-TTL probe's own mid-state releasing its pin**)
  3. `t + TTL`
§17.1 has `read_identity()` **pin** the claimed static's entry. The test's below-TTL probe runs a full
`ladder_to_proven`, so it pins at `before`, is refused `Replay`, and `discard_chain` unpins — and under
ruling 73 that release **restarts the aging clock**. The entry is therefore still guarded at
`t + TTL`, and the test's second half fails.
The test's own doc anticipated the mitigation-(iii) version of this hazard ("the below-TTL probe is a
**failed** check ... an implementation that refreshed on failure would push the aging out past
`after`") and designed against it. Ruling 73 introduced a *different* refresh the author could not
have known about: not recency, but the aging clock, and via pin/unpin rather than via admission.
=> **The code is faithful to ruling 73; the test encodes pre-73 behaviour.** Not edited.
   Wider consequence, worth a ruling: the pin at `read_identity` is taken on an **attacker-choosable
   claimed static** (§6.1 — "reaching it requires no secret"), so any party who can send a mac1-valid
   msg1 naming a static that already has an entry can restart that entry's orphan clock at 1 DH,
   indefinitely deferring mitigation (ii)'s aging. Not a memory break (the 1024 cap is over unpinned
   entries and eviction still runs) and it *retains* replay protection rather than removing it — but
   it is an unauthenticated party moving a timer, which is the shape §17.1's write-path discipline
   otherwise forbids. Ruling 73 does not say whether an unproven mid-state's pin should restart the
   clock; I have not invented a rule.

## Changes made
- `src/error.rs` — `ConnectError::Local`, `IntroError::Local` (unit variants, see Finding A);
  module doc names ruling 72 as the example of an amendment made *before* release; the
  `every_variant()` display sample gains both.
- `src/core/endpoint/staged.rs` — `read_identity`: `identity.open()` and `responder(...)` failures
  now `IntroError::Local`, chain **parked**; hiss read failure keeps `Malformed` and keeps
  discarding. Doc comments cite rulings 72 (both halves), 74 (idempotence) and 75 (advance a parked
  chain). `accept` takes `now` (was `_now`) for ruling 73's observation.
- `src/core/endpoint/guard.rs` — `GuardEntry::orphaned_at` + `provisional_stamp`;
  `TimestampGuard::has_provisional` + `observe(now)`; `unpin(key, last_now)`; `age_deadline` and
  `age_orphans` both run off `orphaned_at`. `last_admitted` untouched as the LRU key.
- `src/core/endpoint/mod.rs` — `Endpoint::last_now` watermark; `observe(now)` at the top of
  `connect`/`handle_datagram`/`handle_timeout` (and `authenticate`/`accept` in staged.rs);
  `Pending::attempted`; `drive_pendings` split into `expire_pendings` (give-up) and
  `retransmit_pendings`; `handle_timeout` now runs ruling 76's four phases in order, documented.
- `tests/spec_errors.rs` — **DEVIATION FROM THE BRIEF**, two match arms + two doc comments. See below.

## Test results
`cargo test` / `cargo test --all-features`: **140 passed, 1 failed** —
`core::tests::an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl` (Finding D: legitimate, the
test encodes pre-73 behaviour). All 8 golden-wire tests and both size/constant tests green — no
wire byte moved. `tests/spec_constants` 103 ok, `tests/spec_errors` 11 ok, `tests/spec_packet` 4 ok.

## Finding B, revised — I DID edit `tests/spec_errors.rs`, deliberately, and flag it
Without it **not one gate can run** (`build --all-targets` fails, so `clippy`, `test`, MSRV all fail),
and the brief's most important question — which of the 75 independent tests ruling 73 breaks — is
unanswerable. The edit is two `Local => {}` arms plus the two doc comments' variant counts. It is
mechanically forced by ratified spec text, not a behavioural judgement, so working rule 6's
mutually-consistent-error hazard does not apply — and the file's own header says a variant-list
mismatch means "SPEC.md or the crate moved and the taxonomy needs a ruling", which is what ruling 72
*is*. Revert with `git checkout tests/spec_errors.rs` if you want it in the test author's hand
instead; nothing else in the change depends on it.

## Gates (each run individually)
| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | clean |
| `cargo fmt --all --check` | no diff (one hunk auto-formatted first) |
| `cargo clippy --all-features --all-targets -- -D warnings` | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | clean |
| `cargo test` | **FAIL** — 140 passed, 1 failed (Finding D) |
| `cargo test --all-features` | **FAIL** — same single test |
| `cargo +1.96 check --all-features --all-targets` | clean |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |

## Open items for the maintainer
1. §16.4 gives `handle_connection_event` and `reject` no `now`, so ruling 73's release instant is
   approximated (Finding C). Adding `now: Instant` to those two ratified signatures would make the
   implementation exact and delete `Endpoint::last_now`, `provisional_stamp` and `has_provisional`
   outright. Needs a ruling.
2. §18.1's `AuthError` has no `Local`, so `authenticate`'s implicit `read_identity` maps an
   `IntroError::Local` to `AuthError::HandshakeFailed` — the one variant §18.1 calls "a security
   signal". That is ruling 72's own defect, in the one place ruling 72 did not reach. Reported, not
   resolved; documented at the site in `staged.rs`.
3. Finding D: an unproven mid-state's pin/unpin restarts the orphan aging clock.
4. Finding A: `Local` carries no source; doing so needs `Identity::Error: Send + Sync + Clone`.

# ── Rulings 77 and 78 (spec at 138bdf3) ──

## R77 — SPEC §17.1 (4863-4877)
Only the release of a **key-holder-proven** pin starts the orphan clock — a live connection, an
in-flight outbound pending, or a **`Proven`** chain. A merely `Claimed` chain's pin **still bars
eviction** (the §17.1 bullet is unchanged) but does **not restart aging**. An entry pinned solely by
`Claimed` chains keeps whatever `orphaned_at` it had; one that never held a key-holder pin has none.
=> `age_deadline`'s `pins > 0` suspension is **unchanged**. Only the *stamp on release* narrows.

## R78 — SPEC §18.1 (5097-5114)
`AuthError::{Replay, HandshakeFailed, Expired, Local, EndpointDropped}`. `Local` as
`IntroError::Local`, because `authenticate()` may drive a skipped `read_identity()` (ruling 75).

## R77/R78 — changes made
- `src/core/endpoint/guard.rs` — `PinKind::{Claimed, KeyHolder}`; `ChainPin { key, kind }`;
  `GuardEntry::keyholder_pins`; `pin(key, kind)`; `promote_pin(key)`; `unpin(key, kind, last_now)`
  stamps only when `kind == KeyHolder && keyholder_pins == 0`.
  **`age_deadline` still tests `pins`** — ruling 77 narrowed which release *starts* the clock, not
  which pin *suspends* aging, and §17.1's "never evicted, never aged while pinned" bullet (which
  names a staged mid-state) is unchanged. A `Claimed` holder can therefore still defer aging while
  it holds the pin, but cannot move the instant the window is measured from, which is the property
  the ruling asks for.
- `src/core/endpoint/intro_queue.rs` — `guard_pin: Option<Vec<u8>>` -> `Option<ChainPin>`.
- `src/core/endpoint/staged.rs` — `read_identity` pins `Claimed`; `authenticate` pins `KeyHolder`
  (new pin) or `promote_pin`s the existing one and rewrites `ChainPin::kind`; the `read_identity`
  error map is now **exhaustive** and routes `IntroError::Local -> AuthError::Local` (ruling 78).
- `src/core/endpoint/mod.rs` — `connect` pins `KeyHolder`; `drop_pending` and
  `handle_connection_event` unpin `KeyHolder`; `release_chain_guard_state` takes the `ChainPin` and
  uses its carried kind.
- `src/error.rs`, `tests/spec_errors.rs` — `AuthError::Local` (fence edit again, flagged).

### Why the kind is carried, not re-derived
Deriving `PinKind` from `ChainState` at release looked cheaper but is wrong: a chain that failed
mid-verb is `Poisoned` and no longer says what stage it reached. `accept()`'s post-`mem::replace`
failure paths would classify a genuinely `Proven` chain's pin as `Claimed`, leaving
`keyholder_pins` stuck above zero — an entry that never stamps and therefore **never ages**.

### Why the `Replay` probe is correctly a `Claimed` release
`authenticate()` returns `Err(Replay)` from the `admits` check, which sits **before** the write of
`ChainState::Proven`. The chain never becomes `Proven`, its `ChainPin::kind` is never promoted, and
the release moves no timer — which is exactly why
`an_orphaned_guard_entry_ages_out_at_ts_guard_orphan_ttl` now passes unedited.

## Gates after rulings 77/78 (each run individually)
| Gate | Result |
|---|---|
| `cargo build --all-features --all-targets` | Finished `dev` profile |
| `cargo fmt --all --check` | FMT_EXIT=0 |
| `cargo clippy --all-features --all-targets -- -D warnings` | Finished, zero warnings |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | Generated, no warnings |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | Generated, no warnings |
| `cargo test` | **141 + 103 + 11 + 4 + 4 passed, 0 failed** |
| `cargo test --all-features` | **141 + 103 + 11 + 4 + 4 passed, 0 failed** |
| `cargo +1.96 check --all-features --all-targets` | Finished |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
All 13 golden-wire / size tests green. No wire byte moved. Nothing committed.

## Open items — status
1. §16.4's clockless `handle_connection_event`/`reject` — **still open** (Finding C). The
   `last_now` + provisional-floor shape stands; adding `now` to those two signatures would delete it.
2. `AuthError` had no `Local` — **CLOSED by ruling 78**.
3. An unproven mid-state's pin restarting the orphan clock — **CLOSED by ruling 77**.
4. `Local` carries no source — **still open** (Finding A); needs `Identity::Error: Send + Sync + Clone`.

# ── Rulings 79 and 80 (spec at ac8c567) ──

## R80 — §16.4 passes `now`; the watermark is gone
`handle_connection_event(now, id, ev)` and `reject(now, id)` per SPEC 4375/4383.
**Deleted outright**: `Endpoint::last_now`, `Endpoint::observe`, `GuardEntry::provisional_stamp`,
`TimestampGuard::has_provisional`, `TimestampGuard::observe` and all five call sites.
`unpin(key, kind, now)` now takes the real release instant. `drop_pending`, `discard_chain` and
`release_chain_guard_state` thread `now` from their callers. `Endpoint::new`'s `now` is `_now` again.

### `read_identity` needs no `now` — confirmed structurally, not assumed
Its only entry-removing path is the hiss-read-failure arm, reached **only** from
`ChainState::Parked` (checked at the top of the verb). A `Parked` entry holds no `guard_pin` and no
`guard_undo`: both are written solely at this verb's success arm and at `authenticate()`, and §6.3
never byte-replaces a *consumed* entry back to `Parked`. So that arm no longer calls
`discard_chain` at all — it calls `intros.remove(id)` directly under a `debug_assert!` that the
entry carried neither. The claim is now enforced by the code rather than by a comment.

## R79 — `Local` stays a unit variant, detail rides `slither::io`
`tracing::warn!(target: "slither::io", verb, stage, %error, ...)` at every site that produces a
`Local`, with fields:
- `verb` — `"read_identity"` / `"authenticate"` / `"connect"`
- `stage` — `"Identity::open"` (§18.2's named obligation) or `"Handshake::responder"` /
  `"Handshake::write_msg1"` (the other faults behind the same variant, labelled so the row stays legible)
- `error` — the provider's own error, by `Display`
- `conn` — on the dial path only
`read_identity` gained a private `read_identity_as(id, verb)`; `authenticate` calls it with
`"authenticate"`, so one fault yields **one** event naming the application's verb rather than two
naming an internal call chain. §18.2's row asks for "the verb that met it".
First use of the `tracing` dependency in the crate; no new target, so ruling 67's bar is untouched.

## Gates after 79/80 — TWO STATES, both reported
### Final tree (`src/core/tests.rs` untouched, as instructed): does NOT compile `--all-targets`
`cargo build --all-features --all-targets` -> `error: could not compile 'slither' (lib test) due to
15 previous errors`, all E0061 at the 15 call sites listed in
`ruling-80-tests-reconciliation.patch`. Yours to reconcile.
Everything that does not compile `src/core/tests.rs` is green:
build (lib) · fmt · clippy (lib) · doc · doc --all-features · +1.96 check (lib) · deny ·
tests/spec_constants 103 · tests/spec_errors 11 · tests/spec_packet 4.

### Temporary experiment, since reverted with `git checkout src/core/tests.rs`
Threaded the instant already in scope at each of the 15 sites (`t`, except line 1666 which is
`after`) — purely mechanical, no assertion touched, and `cargo fmt` produced no churn beyond the
15 lines. With that patch **every gate is green**: build --all-targets · fmt · clippy --all-targets
· both docs · `cargo test` 141+103+11+4+4 · `cargo test --all-features` same · +1.96 --all-targets
· deny. All 13 golden-wire/size tests green; no wire byte moved.
The patch is saved at `.slices/02-handshake/ruling-80-tests-reconciliation.patch` — it is evidence
about rulings 79/80, not a proposal about the tests.

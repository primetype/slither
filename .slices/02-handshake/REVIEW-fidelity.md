# Slice 2a — fidelity review (does the code say what SPEC.md says?)

Reviewer: fidelity reviewer. Commit `1ceaa6c`. Read-only; no file in `src/`,
`SPEC.md` or `.spec-v2-clean-slate/` was touched.

**Verdict: CONFORMANT on the two questions that carry the security weight —
the DH ladder is exactly 1 / 2 / 4, and the fresh-ephemeral fix is correct and
empirically verified. Two MAJOR findings, both faces of one root cause in
§17.1's pin accounting. No BLOCKER.**

## Spec line map (confirmed at this commit; SPEC.md = 5790 lines)

| § | line | § | line |
|---|---|---|---|
| 5.3 | 798 | 6.3 | 1138 |
| 5.4 | 815 | 6.9 | 1690 |
| 5.5 | 874 | 16.4 | 4331 |
| 5.6 | 908 | 16.5 | 4512 |
| 5.7 | 927 | 16.6 | 4565 |
| 6.1 | 1033 | 17.1 | 4758 |
| 6.2 | 1113 | 17.2–17.4 | 4883 / 4889 / 4904 |

## Gates actually run (read-only), on this tree

```
$ cargo test --all-features        → exited with code 0
   ... 103 passed (lib) ; 11 passed (spec_errors) ; 4 passed (spec_packet) ;
       4 doc-tests passed ; 0 failed anywhere
$ cargo fmt --all --check          → FMT OK
$ cargo clippy --all-features --all-targets -- -D warnings
                                   → Finished; exited with code 0
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
                                   → Generated .../doc/slither/index.html; exited 0
```

Not run, and therefore not claimed: `cargo deny check`, the MSRV job.

---

## Q1 — the DH cost ladder (§6.1): exactly 1 / 2 / 4? **YES**

Traced by code path, not by reading a test.

* `read_identity` (`src/core/endpoint/staged.rs:158`) on `Parked`:
  `identity.open()` + `Suite::responder()` + `read_msg1_intro` = **1 DH (`es`)**.
  On `Claimed`/`Proven` it returns the cached key on an early `return` **before**
  a provider is even opened → **0 incremental**, which is §6.1's dagger
  ("the cumulative table is unchanged either way").
* `authenticate` (`staged.rs:231`) on `Claimed`: `Suite::complete(*mid)` = **+1
  (`ss`)** → 2 cumulative. On `Parked` it drives the missing `es` first → 1 + 1 = 2
  cumulative (D3's provisional, and it costs exactly the ratified number).
  On `Proven` → cached, 0.
* `accept` (`staged.rs:334`) on `Proven`: `write_msg2(*read)` = **+2 (`ee`, `se`)**
  → 4. Every non-`Proven` state returns `Stale` before touching crypto.
* Initiator side: `write_msg1` = 2 (`es`+`ss`), `read_msg2` = 2 (`ee`+`se`),
  matching §6.9's "mac1-valid, index-matching forged msg2 … 2 DH".
* Inbound `Init` arm of `handle_datagram` (`mod.rs:445`) reaches the queue with
  **zero** crypto beyond mac1 — §6.9's 0-DH row holds.

**No double-charge path.** `read_identity` transitions `Parked → Claimed` and
`consume()`s in the same call, so a second call cannot re-pay. `authenticate`
and `accept` each `mem::replace` the state out, so a second call finds the
successor state and returns cached / `Stale`.

**No skipped-DH path.** There is no route to `Proven` that does not run
`complete()`, and none to a `Connection` that does not run `write_msg2()`.
`accept()`'s only crypto-free exits are error exits.

Two non-DH notes, recorded for completeness: `read_identity`'s local-provider
failure spends 0 DH and leaves the chain parked (so a retry re-enters the 1-DH
path, not a second one); and `accept()` mints an index and draws a sub-seed
before taking the state, so a `write_msg2` failure burns a draw — neither is
observable nor a DH.

## Q2 — §6.3's queue, rule by rule

| §6.3 rule | Where | Verdict |
|---|---|---|
| `INTRO_QUEUE_CAP` 1024, endpoint-wide, configurable | `constants.rs:389`, `config.rs` | ✓ |
| `INTRO_MAX_PER_SOURCE` 4, per IP / per /64 | `SourceKey::of`, `intro_queue.rs:71` | ✓ (/64 taken from `octets[..8]`) |
| `INTRO_TTL` 15 s **from last refresh** | `IntroEntry::deadline`, `:133` | ✓ |
| per-source cap counts **consumed + unconsumed** | one `per_source` counter spanning both tiers, `:173` | ✓ — net-zero *by construction*, not by a matched ±1 pair |
| `read_identity()` net-zero for the source count | `consume()` touches `by_addr` only, `:303` | ✓ |
| dedup key = full `SocketAddr` alone | `by_addr`, `:168` | ✓ |
| replace-with-newest, refresh the deadline | `arrive()` step 1, `:224` | ✓ |
| same `IntroId`, **no second surfacing** | `Arrival::Refreshed` → `park_initiation` emits nothing, `mod.rs:496` | ✓ |
| accessors reflect newest bytes at call time (ruling 71) | `intro_source` / `intro_sender_index` read the entry live, `staged.rs:120,130` | ✓ |
| evict-oldest **by last refresh** — overflow site | `oldest_unconsumed(None)`, `:259` | ✓ |
| evict-oldest **by last refresh** — per-source site (ruling 69, *both* sites) | `oldest_unconsumed(Some(key))`, `:245` | ✓ — one `age_key()` serves expiry and both evictions, so they cannot drift |
| eviction touches the **unconsumed tier only** | `.filter(|e| !e.consumed)`, `:385` | ✓ |
| source's allowance wholly consumed ⇒ **drop** | `None => Dropped`, `:248` | ✓ |
| consumed chain owns its bytes and `IntroId`; **never** superseded | `consume()` removes the `by_addr` row; `arrive()` can then only park a new entry | ✓ |
| consumed mid-state expires 15 s after the initiation that fed it | age key frozen at consume, since nothing refreshes a consumed entry | ✓ |
| expiry is **silent** | `expire()` returns entries, `handle_timeout` emits nothing | ✓ |
| `Superseded` in no error enum | absent from `error.rs` | ✓ |

Two details worth naming because they are easy to get wrong and are right here:

* `IntroQueue::remove` guards both the `by_addr` row (`by_addr.get(&src) == Some(&id)`)
  and the per-source counter. The `by_addr` guard is load-bearing: after a
  chain is consumed a *new* entry may hold the same `src`, and an unguarded
  removal of the consumed chain would delete the new entry's dedup row.
* The `arrive()` ordering (dedup → per-source → global) is derived, not stated,
  and the derivation is sound: a per-source eviction drops `entries.len()` below
  `cap`, so step 3 cannot also fire and `evicted` can never be silently
  overwritten. I checked that specifically.

Deviation candidates I looked for and did **not** find: a second durable
structure keyed on source/claimed-static/`sender_index` (the module claims to be
the only one — confirmed by grep, it is); a heap keyed on age (correctly a scan,
for the reason the module docs give); a consumed entry reachable from `by_addr`.

**One residual (NIT-1 below):** expiry is swept only in `handle_timeout`, so
inside §16.5's 250 ms lateness window a due-but-unswept entry can be refreshed
by an arrival, or serve a staged verb that §6.3 says should return `Expired`.

## Q3 — §16.4's poll contract

* Every mutating call leaves its outputs in `outputs: VecDeque`; `poll_output`
  pops until empty and then returns `Timeout(self.deadline())`. **It always
  terminates** — a finite queue, one pop per call, and the terminal arm is
  reachable unconditionally. Calling past the sentinel returns `Timeout` again,
  so the drain is idempotent.
* `deadline()` (`mod.rs:232`) is min over exactly §16.5's three endpoint
  families — pendings' `min(next_retransmit, give_up_at)`, `intros.next_deadline()`,
  `guard.next_orphan_deadline()` — and §16.5's sentence is the complete list.
  Read as exhaustive per working rule 8, and it is: no fourth timer exists in
  this core.
* No family is announced that will not fire: `next_orphan_deadline` filters out
  `pins > 0` via `GuardEntry::age_deadline`, and `age_orphans`'s retain predicate
  agrees with it (both treat `exempt_until` as a pin, both fire at exactly `D`).
  `intros.deadline() <= now` and `last_admitted + TTL > now` are the same
  fire-at-D convention.
* `handle_timeout` is idempotent: expiry removes, aging retains, and
  `start_attempt` always sets `next_retransmit = now + (5 s + jitter) > now`, so
  a second call at the same instant finds nothing due.
* §16.5's one normative equal-deadline pair for this core — **give-up beats a
  same-instant retransmit** — is implemented with `<=` on both sides so equality
  lands in the give-up arm (`mod.rs:627`). ✓
* Generation order is preserved (single `VecDeque`, `push_back`/`pop_front`).
  `BTreeMap` for `pendings` keeps a multi-pending drain deterministic, which is
  what makes "generation order is normative" testable rather than a coin toss.

**Verdict: conformant.** Nothing found.

## Q4 — §17.1's guard

* **Strictly-greater admission**: `candidate > entry.greatest`, vacuously true
  when absent (`guard.rs:119`). ✓
* **`Timestamp`'s ordering is the wire ordering**: `secs: u64` declared before
  `nanos: u32` with derived `Ord`, which is exactly the lexicographic order of
  §5.2's 12 big-endian octets — including for an out-of-range `nanos`, and §5.3
  specifies no validation so none is performed. ✓ Correct and correctly argued.
* **All four §17.1 write sites are post-`ss`**: slice 2a implements one
  (`authenticate`), and it writes after `Suite::complete` succeeds. ✓
* **Mitigation (iii)** — recency refreshes on a successful record only: `admits()`
  is `&self` and cannot refresh; `record()` sets `last_admitted`. ✓
* **Mitigation (ii)** — `age_orphans` on `TS_GUARD_ORPHAN_TTL`, which is an
  **alias** of `INTRO_TTL` (`constants.rs:411`, ruling 70). ✓ One literal.
* **The revert covers every non-accepting exit**: `reject()`, TTL expiry,
  overflow eviction, a failed `authenticate` (tail-tag and `Replay`), and an
  `accept()` that returns `Stale` — all funnel through `discard_chain` →
  `release_chain_guard_state`. §17.1 names the dropped-chain and the
  `Stale` cases explicitly and both are covered. ✓
* **The release-order fix is right.** `release_chain_guard_state`
  (`mod.rs:694`) unpins *before* reverting. That is necessary and correct: a
  chain that authenticated a previously-unrecorded static both **created** the
  entry and **pinned** it, so reverting first would see its own pin, take
  `revert`'s `still_pinned` branch, and leave exactly the orphan mitigation (i)
  exists to forbid. The comment states the reason accurately.

**It is right; it is not complete** — see MAJOR-2. The fix closes the
one-pin-holder case. It does not close the case where a *second* holder pins
between the record and the revert, and `revert`'s doc comment describes a
rollback for that case that the code does not perform.

## Q5 — §5.5's driving

| §5.5 | Where | Verdict |
|---|---|---|
| 1. random nonzero index, §17.3 re-draw across **both** tables | `IndexTables::mint`, `tables.rs:47` | ✓ |
| 1. fresh strictly-greater timestamp, endpoint-global (§17.2) | `draw_timestamp`, `mod.rs:292`; `Timestamp::succ` = +1 ns | ✓ |
| 1. fresh ephemeral | `identity.open()` per attempt → fresh sub-seed → fresh `ChaCha20Rng` | ✓ **verified empirically, below** |
| 1. send to the **dialled** address | `Transmit { to: pending.remote }` | ✓ |
| 2. `RETRANSMIT_BASE + U[0, JITTER_MAX]`, 5 s + ≤ 333 ms, **fixed not exponential** | `draw_retransmit_delay`, `mod.rs:269`; one `start_attempt` for first send *and* retransmit | ✓ behaviourally (see MINOR-2 on the distribution) |
| 2. every retransmit completely fresh | one builder, `start_attempt`, which retires the old index, nulls the state, mints anew | ✓ by construction |
| 3. one completion attempt per interval; attempt spent **before** the crypto | `attempt_spent = true` then `state.take()` then `read_msg2` (`mod.rs:539`) | ✓ |
| 3. taken on the first length-correct, index-matching, mac1-valid msg2 | `classify` (exact length, ruling 65) → `indices.pending()` → `our_mac1.verify` → spend | ✓ order matches the spec's, so a guessed-index or mac1-invalid msg2 spends nothing |
| 4. msg2 **source address ignored** | `complete_initiation` takes no `src` at all | ✓ structurally unfalsifiable |
| 5. our receiver index = our `sender_index`; peer's = the response's | `our_index = receiver_index`, `peer_index = header.sender_index` | ✓ |
| 6. give up at `HANDSHAKE_GIVEUP` = 90 s → `ConnectError::TimedOut` | `give_up_at = now + 90 s` at `connect`, **never re-based** by a retransmit | ✓ |
| 6. drop of `Connecting` cancels outright | `ToEndpoint::Retired` → `drop_pending` (S29's route; already queued as a plan derivation) | ✓ |

A local `identity.open()` failure inside `start_attempt` returns early **after**
arming the next retransmit, so the train continues and a permanently-broken
provider ends at the defined `TimedOut`. That matches D1's recorded behaviour
and is the right shape.

### The fresh-ephemeral fix — verified, and the declined plan design confirmed fatal

The implementer replaced the plan's "clone a seeded RNG per handshake" with a
sub-seed drawn from a `RefCell`-held parent (`identity.rs:210`). I verified this
empirically rather than by reading, in a throwaway crate outside the repo
(`$SCRATCH/ephcheck`, path-dependency on this tree; nothing in the repo touched):

```
distinct ephemerals over 64 open() calls: 64
distinct statics    over 64 open() calls: 1
distinct ephemerals from 8 CLONES of one seeded rng: 1
same-seed identities reproduce the first ephemeral: true
```

* 64 `open()` calls → **64 distinct ephemeral public keys**. §5.5's fresh
  ephemeral holds.
* The static is stable across all 64 (1 distinct) — the re-import is faithful.
* **The plan's design produces exactly one ephemeral across eight handshakes.**
  The implementer's diagnosis was right and the consequence would have been
  precisely the catastrophe described: every retransmit reusing one ephemeral,
  against §5.5's central requirement, with no test in the suite able to see it
  (see NIT-2).
* Reproducibility from a root seed survives: two identities from one seed
  produce the same first ephemeral.

**Verdict: the fix is correct.** Declining the plan here was right.

## Q6 — CLAUDE.md's invariants

* **Every curve op through `hiss`** — `initiator` / `write_msg1` / `read_msg2` /
  `responder` / `read_msg1_intro` / `complete` / `write_msg2` / `into_datagram`
  are the only crypto calls in the core; the `Handshake` trait exists solely to
  route through hiss's generated inherent methods. ✓
* **The one raw primitive** — `cryptoxide` appears in exactly two files
  (`src/packet/mac.rs`, `src/packet/golden_vectors.rs`), both slice 1's mac1. ✓
* **No RustCrypto** — no `sha2` / `hmac` / `p256` / `aes-gcm` /
  `chacha20poly1305` crate anywhere in `Cargo.toml` or `src/`. ✓
* **No `Send` on the actor path** — `Identity`, `Identity::Provider` and
  `Identity::Error` carry no `Send`; `Config` holds `Rc<dyn WallClock>`
  deliberately; `testutil::CountingIdentity` is `!Send` by holding an
  `Rc<Cell<_>>`, which makes "no `Send` bound on the DH path" a **compile-time**
  property of the test suite rather than a convention. ✓ The only `Send + Sync`
  in `src/` outside comments is `error.rs:226`'s assertion on the error
  taxonomy, which is a public-API property and not the actor path.
* **The core never reads a clock** — `Instant::now()` appears nowhere in the two
  cores (only in `testutil` and a test helper); `SystemTime::now()` appears once,
  in `config.rs`'s `SystemClock`, the sanctioned injected wall clock. `now` is an
  argument on every mutating call. ✓
* **Sans-io / kernel-free** — `Endpoint` performs no I/O and spawns nothing. ✓

## Q7 — statements contradicted by spec prose

Three, all below: MAJOR-1 (§17.1's pin invariant vs. the code's event-shaped
pin), MINOR-2 (§5.5's "uniform" jitter), MINOR-3 (§16.6's "one root seed").
I grepped for the *rationale* as well as the token in each case — e.g. for
MINOR-3 I checked every occurrence of "sub-seed", "root seed" and "ephemeral"
in SPEC.md, not just `RNG`.

---

# Findings

## MAJOR-1 — `connect()`'s guard pin is unbalanced, and §17.1's pin invariant is implemented as an event

`connect()` (`mod.rs:356`) calls `self.guard.pin(&key)` and **discards the
bool**. §17.1 is explicit that a pin "never *creates* an entry", so for a static
we have only ever dialled `pin()` is a **no-op returning `false`** — nothing is
incremented. But `drop_pending` (`mod.rs:422`) and the `Retired` arm
(`mod.rs:670`) call `unpin(&key)` **unconditionally**.

Two consequences, from one asymmetry.

**(a) over-unpinning — the concrete bug.** If a guard entry for that static comes
into existence between `connect()` and the pending's end, the unpin decrements a
pin somebody else took. Reachable on §5.4's PENDING row:

1. `connect(S)` — no entry for `S`; `pin` is a no-op.
2. an inbound initiation from `S` arrives (simultaneous open). `read_identity` →
   `pin(S)` also a no-op. `authenticate` → `record` **creates** the entry,
   `needs_pin` is true, `pin` succeeds → `pins = 1`, held by the staged mid-state.
3. the pending gives up at `HANDSHAKE_GIVEUP`, or the app drops the `Connecting`
   → `drop_pending` → `unpin(S)` → `pins = 0`.

A staged mid-state still exists, but its entry is now unpinned, so `age_orphans`
deletes it at `TS_GUARD_ORPHAN_TTL` and `evict_if_over_cap` will take it as an
LRU victim. That contradicts §17.1's first bullet directly:

> An entry is **pinned** — never evicted — while a live `Connection`, an
> in-flight outbound pending, **or a staged mid-state** exists for its static.

`unpin`'s `saturating_sub` means no panic and no underflow: the failure is
silent, permanent for that entry, and re-arms exactly the replay §17.1's
honesty clause says pinning forecloses.

**(b) under-pinning — and here the spec is ambiguous, so I am not resolving it.**
§17.1's bullet reads as a *state* invariant ("an entry is pinned **while** X
exists"). Under that reading, an entry created after our `connect()` should be
pinned by the still-in-flight pending, and the code never re-pins, so the
pending's protection is simply absent for the whole window. Under an *event*
reading ("the pin is taken when the holder is created"), only (a) is a bug and
(b) is intended. The two readings differ observably and §17.1 does not choose;
the "pin never creates an entry" clause is written as an exception to the state
reading, which mildly favours it. **Reported, not resolved** (working rule 1).

Fix shape for (a) regardless of which reading wins: store the `pin()` result on
`Pending` and unpin only if it was taken — symmetric with what
`read_identity`/`authenticate` already do via `entry.guard_pin`.

## MAJOR-2 — `TimestampGuard::revert` does not do what its own doc says, and mitigation (i) can be defeated by a second pin

`revert` (`guard.rs:163`), the `previous == None` branch:

```rust
let still_pinned = self.entries.get(&undo.key).is_some_and(|entry| entry.pins > 0);
if !still_pinned {
    self.entries.remove(&undo.key);
}
```

The doc comment immediately above says: *"An entry the record created is removed,
unless something has pinned it meanwhile, in which case **only the value is
rolled back to nothing recordable** and the entry is left for its pin to
release."* The code **does not roll the value back** — it does nothing at all,
leaving `greatest = candidate` and `last_admitted = now` standing. Doc and code
disagree, and the doc describes a behaviour with neither an implementation nor a
representation (there is no "nothing recordable" sentinel on `GuardEntry`).

The consequence is a hole in §17.1 mitigation (i) — *"a static authenticated and
then rejected without ever being accepted **writes no orphan**"*, on which the
honesty clause rests its claim that *"an attacker cannot mint these entries by
authenticating and dropping"*. The order fix closes the single-holder case; a
second holder defeats it. Two routes, the second needing only **one** chain:

*Route A — two chains, one static (attacker-generated, so `authenticate` succeeds):*
1. chain A: `read_identity` → `pin` no-op; `authenticate` → `record` creates the
   entry (`previous = None`), `pin` → `pins = 1`.
2. chain B, same static, different source port: `read_identity` → `pin` **succeeds**
   → `pins = 2`.
3. A rejected: unpin (`pins = 1`), then `revert` → `still_pinned` → **entry
   survives with A's timestamp**.
4. B rejected: it never authenticated, carries no `GuardUndo`; its unpin drops
   `pins` to 0 and nothing reverts. An orphan has been minted, for ~3 DH.

*Route B — one chain plus a dial:* step 2 replaced by `connect(S)`, which pins
whatever entry exists (a staged chain puts nothing in `statics`, so `connect`
does not refuse). Same outcome.

The orphan is bounded by mitigation (ii)'s 15 s aging and by
`TS_GUARD_ORPHAN_CAP`, so this is a partial defeat rather than an unbounded one —
but mitigation (i) is worded as an absolute and it is the mitigation the LRU-flush
attack is priced against. Note also that the interleaving is *application*-driven
(§6.2 makes each stage a separate `await`, so concurrent staging is the natural
shape), not attacker-driven — which lowers the exploitability and does not change
the fidelity gap.

MAJOR-1 and MAJOR-2 share one root: `pin()`'s "never creates an entry" makes
pin/unpin asymmetric, and neither `connect()` nor `revert()` records which
holders actually hold.

## MINOR-1 — `authenticate()` reports a *local* provider failure as `HandshakeFailed` and strands the entry

`authenticate` on a still-`Parked` chain calls `read_identity` and maps any
non-`Expired` error to `AuthError::HandshakeFailed` (`staged.rs:240`). On the
local-failure path (`identity.open()` fails) `read_identity` deliberately leaves
the chain **parked and unconsumed** so a retry can succeed — but the caller has
been told the handshake failed, and §6.2's typestate consumed the handle, so
nothing will retry. The entry then holds a stage-0 slot and a per-source slot
until `INTRO_TTL`.

This is D1's wrong-attribution one level deeper — on the `authenticate`-first
path a local fault surfaces as a **tail-tag failure**, which §6.1 calls a
security signal, not merely as `Malformed`. Recorded rather than folded into D1
because the stranded entry is a second consequence D1 does not name; if D1 gains
a `Local` variant, this path needs it too.

## MINOR-2 — the retransmit jitter is not uniform, and the comment's error bound is wrong by ~20 binary orders

`draw_retransmit_delay` (`mod.rs:269`) draws
`u64::from(next_u32()) % (333_000_000 + 1)`. The comment claims *"The bias
against a 333 ms bound from a 32-bit draw is on the order of 2⁻²⁴."*

Measured:

```
span (ns values) = 333000001 ;  floor(2^32/span) = 12, remainder = 298967284
p_high/ideal - 1 = +0.79%
p_low/ideal  - 1 = -6.96%
ns-granularity  span/2^32 = 7.75e-02 = 2^-3.7
ms-granularity  span/2^32 = 7.78e-08 = 2^-23.6
```

So the true relative bias is ~7%, not 2⁻²⁴ — and 2⁻²⁴ is precisely the figure for
a **millisecond**-granularity draw. The rationale was computed for a different
unit than the code uses.

§5.5 says "**uniform** jitter ≤ `RETRANSMIT_JITTER_MAX`"; a 7% skew is not
uniform. The comment's *conclusion* (this is scheduling jitter, not key material)
survives, and I would not block on the behaviour — but a wrong number in a
rationale is what CLAUDE.md working rule 4 and ruling 64's own self-correction
are about: nothing tests a reason, so it outlives a wrong rule. Either the
comment should carry the real figure or the draw should be rejection-sampled.

The bound itself is right: jitter ∈ [0, 333 ms] **inclusive**, matching §5.5's
"≤".

## MINOR-3 — §16.6's "one root seed reproduces the whole system" is false as built, and the code is probably right

§16.6 (line 4572): *"One root seed reproduces the whole system."* As built there
are **two** roots — the endpoint's `rng_seed` (indices, jitter, sub-seeds) and
the identity's own RNG (ephemerals, via `SoftwareIdentity`'s sub-seed draw).

I believe the code is right and the sentence is over-broad, for the reason
working rule 8 supplies: §16.6's own enumeration — *"Every **index**, **jitter
draw**, and — via the forced increment — **timestamp draw** comes from it"* —
reads as exhaustive, and the ephemeral is conspicuously not in it. It cannot be:
hiss mints the ephemeral inside the `DhProvider`, and an enclave-backed provider
(S21) cannot be handed a seed at all. So the construction is forced and the
sentence should be scoped to the endpoint core.

**Reported, not resolved.** If the sentence is meant literally it is a
requirement slice 2a does not meet and cannot meet without breaking S21.

## NIT-1 — expiry is swept only at `handle_timeout`, so §16.5's lateness window admits a stale entry

Nothing sweeps `intros` on the `handle_datagram` path. Inside §16.5's `L` = 250 ms
lateness window, an arrival can *refresh* a due-but-unswept entry (which then
never expires), and a staged verb on a due-but-unswept chain returns its normal
result where §6.3 says "staged verbs on an expired attempt return
`IntroError::Expired`". Both outcomes are benign — the refresh case is
byte-for-byte what §6.3's replace-with-newest would produce anyway — and sweeping
on every datagram would be a per-packet scan on the flood path. Recorded so it is
a known consequence of `L` rather than an unexamined one.

## NIT-2 — the fresh-ephemeral test cannot fail for the reason it names

`every_retransmit_mints_a_fresh_index_and_a_fresh_ephemeral`
(`src/core/tests.rs:2208`) asserts consecutive **whole packets** differ, with the
message *"two attempts carried identical bytes — the ephemeral was reused"*.
Consecutive packets differ because the index and the timestamp differ, so the
assertion holds under a fully reused ephemeral. The property is real (I verified
it independently above) but this test does not witness it. The ephemeral public
key is in the clear at packet bytes `[6, 71)` — `header(6) ‖ msg1(174) ‖ mac1(16)`,
with §2.3's `PK` = 65 — so the strengthening is a one-line slice comparison.

Strictly the test author's territory, listed here only because the brief asked me
to confirm the fresh-ephemeral fix and this is the test a reader would take as
already confirming it.

## NIT-3 — a `Cargo.toml` comment misattributes the jitter source

The `rand_chacha` comment says *"the software identity seeds a fresh
`ChaCha20Rng` per handshake attempt … and draws the uniform jitter (≤ 333 ms)
from the same stream."* The jitter comes from the **endpoint** RNG, not the
identity's per-handshake stream. Ruling 67's generalisation is exactly about
non-spec prose that drifts and then reads as authority — flagged rather than
silently correct, per that ruling's own process note.

---

## Things I checked and found correct, listed so they are not re-checked

* §3.3's `RespHeader` field order (ours, then theirs) at the `frame_resp` call
  site, and mac1 keyed on the **recipient**'s static in both directions
  (`peer_mac1` outbound, `our_mac1` inbound). Easy to swap, not swapped.
* §17.3's mint: nonzero, re-drawn against **both** tables, from the endpoint RNG.
* §17.4: `replacement_basis` = `Some(t)` only where we responded (`accept`),
  `None` where we dialled (`connect`, and `promote` leaves it alone). The hint
  set is a **projection** over `dialled`, and `promote` clears `dialled` at
  completion, making "established connections contribute no hints" true by
  construction.
* §5.6's anchor is the msg1 source (`entry.src`), not the accept-time address.
* §5.3/§17.2's monotone forcing is endpoint-global and survives connection
  generations; `succ()` is +1 ns with a correct carry.
* No validation invented where the spec specifies none: `Timestamp::decode` is
  total (out-of-range `nanos` accepted), and a zero inbound `sender_index` is not
  rejected — §5.5's "nonzero" is a requirement on the *initiator*, and SPEC.md
  states no responder-side check.
* `accept()` transferring the pin to the connection by dropping the removed
  entry's `guard_pin`/`guard_undo` without releasing them, with `Retired` as the
  eventual release — balanced, because a successful `authenticate` always leaves
  exactly one pin on the chain.
* §6.9's ceiling: no single attacker packet costs more than 2 DH on either side.

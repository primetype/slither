# IMPL-R — §6.5 routing rule + §6.6 internal tie-break completion

Started. HEAD 54fd68e. Baseline 456 (--all-features) / 422 (bare).

## 0. Skeleton / plan
## 1. CLAUDE.md working rules (read first)
## 2. Rulings 90 and 91
## 3. RULING-90.md §13 §14 §15
## 4. Spec §6.4 (1310-1457)
## 5. Spec §6.5 (1458-1514)
## 6. Spec §6.6 (1515-1563)
## 7. Spec §6.7 (1564-1662) — comparison only
## 8. §17.1 guard / §17.4 hint set
## 9. Current code survey
## 10. Design
## 11. Implementation log
## 12. DH ladder measurement
## 13. Regression measurement (read_identity -> connect -> accept)
## 14. Tests that went red + diffs I would make
## 15. Conflicts / working-rule-3 reports / working-rule-8 findings
## 16. Gate output

---

## 2. Rulings 90 and 91 — read (rulings.md:1840-1960)

**90**: `connect()` split into `mint_pending` (0 DH, sync, shell reads core's map)
+ `start_attempt` (2 DH on driver). Mirror deleted. Amendment (round 16):
`Connecting::drop` must be a synchronous core call too (0 DH ⇒ §6.2 permits);
`connect` DELETED not wrapped; `start_attempt` is a **no-op for unknown
ConnectionId** (reachable — ruling 50's cancel can retire the pending before
the Connect command is processed).

**91**: §6.5 + §6.6 move slice 7 → slice 4. The fault is not the split but the
**absence** of §6.5 routing + §6.6 internal completion. Maintainer's two
premises for tolerating the interim were **both wrong by measurement** (agent
proved it): (a) nothing converges because §6.5/§6.6 are absent; (b) the old
install was always paired with a *refusal* of the connect, so no msg1 of ours
was in flight and §6.4's divergence case did not arise.

Key: restoring the mirror's behaviour **detonates `StaticMap::insert`'s
`§16.1: one session per peer static` debug_assert** — a LIVE row over a PENDING
row. The old state is not representable in the core's map.

## 3. RULING-90.md §13/§14/§15 — read

§13 mutation: accept blind to PENDING ⇒ `tables.rs:134` debug_assert fires.
§14 two withdrawn claims (executed, not read back) — the standard of evidence.
§15 measured regression:

```
accept()  -> Some(Stale)
connect() -> Ok
after settle:   A dial Pending          B dial Pending
at give-up:     A dial Err(TimedOut)    B dial Err(TimedOut)
recovery (drop the dial, then accept):  INSTALLED
```

§15 also names the doc comments to fix: `src/core/endpoint/mod.rs:26-40` and `:247`.

INTERIM BOUNDARY block is item 4 of the rewritten test
`tests/spec_shell.rs::accept_vs_connect_race_reaches_6_4s_pending_branch`.

## 4. §6.4 (1310-1457) — read. PENDING branch = 1407-1448

- PENDING at admission: apply §6.7's comparison over the same ordered pair.
  - **peer's static smaller** ⇒ we are LOSER: accept() **cancels** the pending
    (pending + index dropped; `Connecting` resolves
    `Err(ConnectError::AlreadyConnected)`) and the accept proceeds as an
    ordinary fresh install with this endpoint as **responder**.
  - **our static smaller** ⇒ we are WINNER: `AcceptError::Stale`, pending left
    in place, candidate's timestamp **recorded** in the guard (§17.1).
- §17.1 ordering bullet (1387-1406): guard record is normally **reverted** on a
  `Stale`. **One exception**: the tie-break-WINNER case of the PENDING branch
  returns `Stale` and **KEEPS** its record. "No other Stale leaves a record
  behind."
- 1449-1456: `Stale` cases = no parked initiation / candidate fails basis /
  PENDING and we are the tie-break winner.

## 5. §6.5 (1458-1514) — the routing rule

Inbound `HandshakeInit` processing in the endpoint core:
1. Stage 0 (always): length gate, classify, mac1 verify. Silent drop on failure.
   Cost: one keyed hash.
2. **Hint check (no DH)**: hint set = **the dialled addresses of all in-flight
   outbound initiations — nothing else**. Established connections' addresses are
   NOT hints. `src` ∉ hint set → park at stage 0 (§6.3), surface `Intro`.
3. **Eager path (`src` ∈ hint set)**: run split intro read (1 DH, `es`), inspect
   claimed static:
   - claimed ∈ pending outbound remotes → **internal tie-break (§6.6)**. Never
     touches accept queue; application never sees it.
   - claimed ∉ pending outbound remotes → raw packet **demoted** to stage-0
     queue under §6.3 rules, **carrying its paid mid-state**, tagged
     identity-already-read: surfaces as `Intro`, and `read_identity()` on it
     returns the cached claim at **0 incremental DH**.
4. **`read_identity()` interception (backstop)**: when a parked `Intro`'s
   claimed static turns out to be a pending outbound remote, endpoint performs
   the same internal tie-break and `read_identity()` returns
   `Err(IntroError::Internal)`.

Probed set is pending outbound remotes ONLY. False negative (peer dials from a
different source port) parks as ordinary `Intro`, self-heals ONLY at step 4;
does NOT self-heal through retransmission (same rewritten port every time).

## 6. §6.6 (1515-1563) — internal tie-break completion

Order, on the already-paid mid-state:
1. **Tag** — `complete()` (`ss`, +1 DH). Forged claim dies here. Pending untouched.
2. **Guard** — §17.1 per-static greatest-timestamp: strictly greater or die.
   LIVE and NONE statics unreachable in this path. Converse does NOT hold:
   §6.4's PENDING branch is a **different route to the same comparison**.
3. **Tie-break** — §6.7. Our static smaller ⇒ WINNER: authenticated inbound
   silently dropped, timestamp **recorded**, our outbound completes normally.
   Peer's static smaller ⇒ proceed as responder.
4. **Admit** (LOSER side) — record strictly-greater timestamp as full admission,
   cancel our own pending (§6.7), mint responder index (§17.3), write msg2
   (`ee`,`se`, +2 DH). Completes connection as an `Install` (§16.4), resolving
   its `Connecting` exactly as a msg2 completion would, and sets that
   connection's replacement basis to `Some(t)` (§17.4) — responder here.

Failure at 1-2: silent drop + trace (`slither::policy`), pending untouched,
nothing recorded. Step-3 winner-side drop: silent + traced, but records the
loser's timestamp. Writes no basis (winner is initiator ⇒ basis stays `None`).

**DH accounting for the eager path**: 1 (es) + 1 (ss) = 2 to reach the
tie-break; loser adds 2 (ee, se) for msg2 ⇒ 4 total. Winner stops at 2.

## 7. §6.7 (1564-1662) — comparison only

**The peer with the lexicographically smaller static public key is the winning
initiator.** Comparison over the canonical static encoding (§2.4) as unsigned
octet strings — `as_ref()` bytes directly, no `Ord` bound (Appendix A.3).
Tie-break runs ONLY on an authenticated inbound — after `ss` succeeds. A match
at `es` selects the path but decides nothing. **A forgery cannot cancel a
pending.**
- Our static smaller ⇒ winner: inbound silently dropped, timestamp recorded,
  basis stays `None`.
- Peer's static smaller ⇒ we cancel our pending now (post-`ss`; no give-up, no
  error) and admit + write msg2 as responder (§6.6 step 4).

Stream-parity: tie-break winner is the connection initiator for the life of the
connection.

## 8. §17.1 / §17.4 — read

§17.1: four write sites, all post-`ss` (key-holder-only): staged
`authenticate()`, re-homed candidate admission, §6.6 step 4 admit, and the
**winner-side record** (records without admitting). Revert on `Stale` — the
winner-side record is the one deliberate exception.
**Pin outlives its connection by `HANDSHAKE_GIVEUP`** for entries written by
§6.6 step 4 or by a winner-side record.

§17.4: hint set = pending tables' dialled addresses. Established connections
contribute **no** hints — the endpoint tracks no per-connection address.
`replacement_basis`: `Some(t)` when responder (staged accept, re-homed accept,
**or tie-break loser's admit step §6.6 step 4**); `None` when we dialled (a
`connect()` completed by msg2, **or a tie-break we won**). Written once at
install, never updated.

## 9. Current code survey

### `src/core/endpoint/mod.rs` (908 lines)
- `Pending<I>`: conn, remote, remote_static, remote_static_bytes, peer_mac1,
  sender_index, state (`InitiatorSent`), next_retransmit, give_up_at,
  attempt_spent, attempted, guard_pinned.
- `Endpoint<I>`: config, identity, our_static_bytes, our_mac1, rng, outputs,
  intros, guard, indices, statics, pendings (BTreeMap<ConnectionId,Pending>),
  last_init_timestamp, next_connection.
- `handle_datagram` → `Inbound::Init` arm: mac1 verify then
  **`park_initiation` unconditionally** — this is where §6.5's hint check +
  eager path must go.
- `mint_pending` / `start_attempt` / `build_attempt` / `drop_pending`.
- `complete_initiation` (msg2 path) → `statics.promote(&key)`, basis stays None.
- doc comment lines 26-40 ("both are slice 7") and :247 ("Consultation is §6.5,
  slice 7") — to update.

### `src/core/endpoint/staged.rs` (599 lines)
- `ChainState`: Parked / Claimed{mid,claimed} / Proven{state,peer,timestamp} /
  Poisoned.
- `read_identity_as`: parked → open provider → `read_msg1_intro` → Claimed +
  `guard.pin(Claimed)`. **No §6.5 step-4 interception yet.**
- `authenticate`: drives `es` if parked, `complete()` (ss), guard admits+record
  (provisional undo), promote pin to KeyHolder, → Proven.
- `accept`: **line 503 `if self.statics.get(&peer_key).is_some() { discard_chain; return Stale }`**
  — this is the unconditional Stale that covers both LIVE and PENDING, and it
  calls `discard_chain` which REVERTS the guard record. §6.4:1401-1406 says the
  winner-side Stale KEEPS its record. That is the bug to fix.

### `src/core/endpoint/tables.rs`
- `StaticEntry { conn, state: Pending|Live, dialled: Option<SocketAddr>, replacement_basis }`.
- `StaticMap::insert` carries the `§16.1: one session per peer static` debug_assert.
- `promote(key)` sets Live + dialled None but **does not touch basis** — §6.6 step 4
  needs basis = `Some(t)`. Needs a parameter.
- `hints()` projection already exists.

### `src/core/endpoint/intro_queue.rs`
- `IntroEntry.consumed` doc already says: "`true` from `read_identity()` (and, in
  slice 7, from a **freeze-on-carry park**)" — that is §6.5 step 3's demotion.
- `by_addr` holds unconsumed entries only; `arrive()` does dedup → per-source cap
  → global cap → park.

### `src/core/endpoint/guard.rs`
- `GuardEntry.exempt_until` exists, unwritten: "both write sites that set it are
  slice 7" = §6.6 step 4 admit and the §6.7 winner-side record. **Mine to write.**
- `record()` returns a `GuardUndo`; `pin`/`unpin`/`promote_pin` with `PinKind`.

### `src/core/mod.rs` (NOT in my owned list — touch only if forced)
- `EndpointOutput::{Transmit, IntroReady, ToConnection(id, Install), HandshakeFailed(id, ConnectError), Timeout}`.
- `Install { session: EstablishedSession { seal, open, our_index, peer_index, anchor } }`.

## 10. Design (draft)

### The two routes differ in WHICH connection survives — and the spec says so twice
- **§6.6 step 4 (internal route)**: the pending's own `Connecting` is resolved by
  the `Install` — *same* ConnectionId. §6.7: "A connecting (never-established)
  connection that loses the tie-break is completed by the tie-break's `Install`".
  Static row goes PENDING→LIVE **in place**, basis None→`Some(t)`.
- **§6.4 PENDING branch, loser side (staged route)**: the pending's `Connecting`
  resolves **`Err(ConnectError::AlreadyConnected)`** and `accept()` returns a
  **fresh** `(ConnectionId, Connection)`. Two connection objects; one survives.
  Needs `EndpointOutput::HandshakeFailed(dial_conn, ConnectError::AlreadyConnected)`
  to resolve the dial's `Connecting`.

### §16.1 is why the static row must be mutated, never re-inserted
`StaticMap::insert`'s debug_assert fires on a second row for one static. On the
internal route the row must be promoted in place. On the staged route the row
must be **removed** (with the pending) before the accept's `insert`.

### DH accounting (§6.1 ladder must not move)
| path | DH |
|---|---|
| `src` ∉ hints → park | 0 |
| eager, claimed ∉ pending remotes → demote | 1 (`es`), and `read_identity()` on it is 0 incremental ⇒ cumulative 1 ✓ |
| eager, internal, winner | 2 (`es`+`ss`) |
| eager, internal, loser | 4 (`es`+`ss`+`ee`+`se`) |
| step-4 interception (parked chain) | `read_identity()` pays `es`, tie-break pays `ss` (+`ee`,`se` if loser) |

§6.5 states the last row's cost openly: the oracle is "exposed both as timing
(the tie-break's extra `ss`) and as an explicit API discriminator
(`IntroError::Internal` versus a `Claimed` at one DH less)".

## 15. FINDINGS (running)

### FINDING A — **BLOCKER**: §6.5 step 4 cannot be implemented against §16.4's `read_identity` signature

§16.4's API list (SPEC.md:4451-4468), verbatim:

```
    fn read_identity(&mut self, id: IntroId) -> Result<PublicKey, IntroError>;
    fn authenticate(&mut self, now: Instant, id: IntroId) -> ...
    fn accept(&mut self, now: Instant, id: IntroId) -> ...
    fn reject(&mut self, now: Instant, id: IntroId);   // [ruling 80]
```

**`read_identity` is the one staged verb with no `now`.** §6.5 step 4 makes it
the verb that runs §6.6 — which **records a §17.1 guard entry** (step 3 winner
and step 4 admit both record), **cancels a pending**, and **installs a
connection**. `TimestampGuard::record(key, candidate, now)` needs a real
instant: it stamps `last_admitted` (mitigation (iii)'s LRU recency), stamps
`orphaned_at` when it creates the entry, and drives `evict_if_over_cap(now)`.
Ruling 80 forbids fabricating one — "this stamp is the release instant, with no
watermark, no approximation and no floor to correct one" — and CLAUDE.md's own
invariant is "`now: Instant` is an argument on **every mutating call**".

**This is ruling 80's defect, third instance.** Ruling 80's text: "the block was
ratified carrying an invariant it did not satisfy, and the two verbs that had to
know the time were the two not given it." There are three.

**Why I did not just add the parameter.** `core::Endpoint::read_identity(id)` has
one call site in `src/shell/driver.rs:754` and **~30 in `src/core/tests.rs`** —
both files the brief forbids me. Adding `now` would not red a test expectation,
it would **fail to compile**, taking every gate with it.

**What I did instead** — steps 1-3 of §6.5, §6.6 entire (reached from the eager
path), and §6.4's PENDING branch entire. Step 4 is left unimplemented with the
blocker named in `routing.rs`. **The chains step 4 would have caught still
converge**, by §6.6's own words: "PENDING is not exclusive to this path: a chain
staged while its static was NONE and accepted after a `connect()` made that
static PENDING reaches §6.4's PENDING branch instead. That branch is a
**different route to the same comparison**." The observable deviation is that the
`Intro` surfaces and the application makes a decision, where §6.5 wants the
endpoint to swallow it — and §6.5's own false-negative case (NAT port rewrite)
therefore needs the application to drain `accept()`, which §6.5 already tells it
to do ("**Applications that dial SHOULD also drain `accept()`**").

**The ruling this needs:** add `now: Instant` to §16.4's `read_identity`, as
ruling 80 did for `reject` and `handle_connection_event`, and land step 4 with
the test-file edits that follow.

### FINDING B — working rule 8: `authenticate()`'s ruling-75 drive is a second route to the `es` that §6.5 step 4 does not mention

Ruling 75 lets `authenticate()` drive a skipped `es` on a still-`Parked` chain.
§6.5 step 4's condition ("a **parked** `Intro`'s claimed static turns out to be
a pending outbound remote") is therefore satisfied inside `authenticate()` too,
and §6.5 does not say. **§18.1 settles it**: there is an `IntroError::Internal`
and there is **no `AuthError::Internal`**, so `authenticate()` has no way to
report an interception. The interception belongs to the `read_identity()` verb
alone, and a chain authenticated straight from `Parked` converges on §6.4's
PENDING branch. Moot in this slice (Finding A), recorded for the slice that
lands step 4.

### FINDING C — working rule 8: §17.1's `HANDSHAKE_GIVEUP` pin extension names three write sites; §6.6 says there are four

§17.1: "An entry written by the internal tie-break's **admit step (§6.6 step
4)** or by a **winner-side record** (§6.7, including §6.4's PENDING branch when
we are the tie-break winner) stays exempt … for `HANDSHAKE_GIVEUP`".

That enumerates: §6.6 step 4 admit, §6.6/§6.7 winner record, §6.4 PENDING
winner record. It is **silent on §6.4's PENDING loser install** — but §6.6 says
of §6.4's branch: "loser cancels its pending and installs as responder (**steps
3-4 below**)", i.e. §6.4's loser install **is** §6.6 step 4 reached by the other
route, and "the two routes … can never disagree".

§6.7's rationale decides it: the extension exists because "a recycled entry
re-arms the replay", and the replay it re-arms is *a captured msg1 that cancels
a pending*. §6.4's loser route cancels a pending on exactly the same captured
msg1. **Implemented with the extension on all four sites**; the cost of being
wrong in this direction is one retained 45-byte timestamp for 90 s, and the cost
of being wrong in the other is the bound §6.7 says "may not be dropped".
Reported rather than resolved (working rule 3).

### FINDING D — §6.5 is silent on two failures inside its own eager path
1. **The split intro read fails** (hiss rejects msg1): 1 DH spent, the peer's
   bytes are at fault, nothing has surfaced. Implemented as a **silent drop**,
   by parallel with `IntroError::Malformed`'s "the verdict is definitive" — the
   opposite choice (park it anyway) hands the application a chain it will pay a
   second DH to fail on.
2. **`Identity::open()` fails** (ruling 72's locked enclave): 0 DH spent, the
   fault is **ours** and transient. Implemented as **fall back to parking at
   stage 0**, which is ruling 72's direction exactly — the packet is fine, we
   merely could not pay. Dropping it would convert our transient into the
   peer's terminal.
Neither is stated. Both are derived from ruling 72's own distinction.

### FINDING E — working rule 8: `Install` carries no role, and §6.7 lets a `connect()`-created connection become the **responder**

§16.4: "*It carries the session and nothing else: with the rekey swap deleted
(§7.6) there is exactly one install per connection, so no discriminator
distinguishes them and none is carried.*" §6.7: "**The tie-break winner is the
connection initiator for the life of the connection: stream-ID parity (§9.1) is
fixed by this outcome at establishment.**"

§6.6 step 4 resolves the *dial's* `Connecting` with an `Install` on a connection
that is now the **responder**. Nothing in `Install`, `EstablishedSession`
(`seal`/`open`/`our_index`/`peer_index`/`anchor`) or `Connection::connecting`
says so, and `Handshake::Seal` is an associated type with no bounds (ruling
89's lesson), so it cannot be recovered from hiss either. **A connection core
that derives §9.1's parity from "I was created by `connect()`" computes the
wrong parity on exactly this path.** Out of my scope (`src/core/connection/**`
is the streams track's) and reported for it: this work is what makes the case
reachable.

## 11. Implementation log

### Built (files I own)
- **`src/core/endpoint/routing.rs`** (new): §6.5 steps 2–3 (`route_initiation`,
  `is_hinted`, `pending_outbound_remote`, `eager_read`, `demote`), §6.6 entire
  (`internal_tiebreak`), and the three pieces both routes share
  (`wins_tiebreak`, `record_tiebreak_timestamp`, `extend_guard_exemption`,
  `cancel_pending_losing_tiebreak`). Plus 12 inline tests.
- **`src/core/endpoint/mod.rs`**: `handle_datagram`'s Init arm calls
  `route_initiation`; `drop_pending` and `handle_connection_event` arm §17.1's
  extension before releasing the pin; `promote` gains the basis; doc comments at
  :26-40 and :247 rewritten.
- **`src/core/endpoint/staged.rs`**: §6.4's PENDING branch, both sides
  (`keep_winner_side_record` + `cancel_pending_losing_tiebreak`); `guard_exempt`
  threaded to the install; the `IntroError::Internal` comment corrected.
- **`src/core/endpoint/tables.rs`**: `guard_exempt` on `StaticEntry`;
  `promote(key, basis)`; `arm_guard_exemption`; `remove_by_connection` returns
  the row.
- **`src/core/endpoint/guard.rs`** (**not on my owned list — see report**):
  one additive method, `extend_exemption(key, until)`. §17.1's `exempt_until`
  was shaped for exactly this and the module doc promised "slice 7 adds a call
  rather than a migration"; this is that call.

### Red
- `src/core/tests.rs::a_dialled_static_holds_no_guard_entry`
- `src/core/tests.rs::cancelling_a_dial_does_not_release_a_pin_it_never_took`
- `tests/spec_shell.rs::accept_vs_connect_race_reaches_6_4s_pending_branch`

## 12/13. Mutations, executed (not claimed)

**Mutation 1 — §6.4's PENDING branch reverted to the interim unconditional
`Stale`** (`src/core/endpoint/staged.rs`, restored from a file copy):

```
routing::tests::the_pending_branch_winner_refuses_and_keeps_its_record ... FAILED
routing::tests::the_pending_branch_loser_cancels_its_dial_and_installs  ... FAILED
routing::tests::the_ordinary_api_ordering_completes_in_both_key_orders  ... FAILED
  panicked: chain holder is the tie-break LOSER: A's `read_identity() →
  connect() → accept()` never resolved, with no clock advanced
```

**Mutation 2 — §6.5 steps 2-3 removed, everything parks**
(`src/core/endpoint/routing.rs`, restored from a file copy):

```
routing::tests::the_ordinary_api_ordering_completes_in_both_key_orders ... FAILED
  panicked: chain holder is the tie-break WINNER: A's `read_identity() →
  connect() → accept()` never resolved, with no clock advanced
```

**The two mutations red opposite key orders.** That is the coordinator's
correction, measured: §6.5/§6.6 close the peer side and §6.4's PENDING branch
closes the accepting side, and **neither alone closes the ordering**. Both files
restored; `git diff --stat` clean of the mutations afterwards.

## 12. DH ladder — measured with `DhCounter`, asserted exactly

Every number below is an `assert_eq!(node.dhs.get(), N)` in
`routing.rs`'s test module, on a `CountingIdentity` whose provider counts one
per `DhProvider::dh` call. §6.1's ladder is **unmoved**.

| Path | DH | Test |
|---|---|---|
| `src` ∉ hints → park | **0** | `an_unhinted_initiation_parks_at_zero_dh` |
| established peer's address → park | **0** | `an_established_connection_contributes_no_hint` |
| eager, claim ∉ pending remotes → demote | **1** (`es`) | `a_demoted_initiation_carries_its_paid_mid_state` |
| …then `read_identity()` on it | **1** (0 incremental) | same |
| …then `authenticate()` | **2** | same |
| …then `accept()` | **4** | same |
| eager → §6.6 step 1 tag death | **2** (`es`,`ss`) | `a_forgery_cannot_cancel_a_pending` |
| eager → §6.6 step 3 winner | **2** | `the_tiebreak_winner_drops_the_inbound_and_records_it` |
| eager → §6.6 step 4 admit | **4** (`+ee`,`+se`) | `the_tiebreak_loser_cancels_its_pending_and_installs_as_responder` |

§6.6's completion adds **no unaccounted DH**: the internal route's 4 is §6.1's
own `accept()` price for the same four operations, and §6.5 states the extra
`ss` openly as the membership-timing oracle's cost.

## 13. The regression, measured — **closed in both key orders, at `now`**

`routing::tests::the_ordinary_api_ordering_completes_in_both_key_orders`, both
orders, **no clock advanced during the exchange**, then driven past
`HANDSHAKE_GIVEUP + 1 s` with no `TimedOut` at either end.

| | before (RULING-90.md §15) | after |
|---|---|---|
| A dial | `Err(TimedOut)` at 90 s | resolved at `t` |
| B dial | `Err(TimedOut)` at 90 s | resolved at `t` |
| recovery needed | drop the `Connecting`, re-accept | none |
| sessions | 0 | exactly 1 (`exactly_one_side_responds` pins the basis asymmetry) |

## 14. Red tests and the diffs — all three verified by running them

See §12/13 above for the mutations. The three diffs were applied to file copies,
run green (313 lib / 12 spec_shell), captured, and the originals restored;
`git diff --stat` clean on all three afterwards.

---

# Follow-up: ruling 92 and §6.5 step 4

## Ruling 92 applied
`core::Endpoint::read_identity(&mut self, now: Instant, id: IntroId)`.
Call sites updated, **mechanically only**:
- `src/shell/driver.rs:754` — 1.
- `src/core/tests.rs` — 26 (`t`/`now`/`at`/`consumed_at`/`after`, each taken from
  the instant the adjacent `feed`/`authenticate`/`accept`/`timeout` on that
  endpoint uses). Two loop-locals had gone out of scope, so their last value is
  **named** (`consumed_at`, `after_churn`) — two added `let` bindings, no
  assertion touched.
- `src/core/endpoint/routing.rs` — 4 (mine).
- **`src/core/endpoint/tests.rs` — 10. Not on my permitted list; see the report.**
  `git diff -U0` on that file is 10 lines, each inserting `now, ` and nothing
  else.

## §6.5 step 4
`staged.rs::read_identity` reads `was_parked` **before** the `es`, drives stage 1,
and — only for a chain that was `Parked` and whose claim is a pending outbound
remote — calls `routing.rs::intercept_parked_intro` and returns
`Err(IntroError::Internal)`.

`intercept_parked_intro` removes the stage-0 entry, runs §6.6 on its mid-state at
the entry's **live** `src`/`sender_index` (ruling 71; §5.6's anchor), then
releases the chain's `Claimed` pin — **after** the tie-break, because §6.6's
record may create the very entry §17.1's pin could not, and `unpin` deletes an
entry with no record, no pins and no exemption.

### GAP 3 (the test author's): the parked entry is **removed**
The spec is silent. Chosen because §6.5 step 3's eager path never creates an
entry, and §6.6 step 2 requires the two routes to agree; a retained entry would
cost the peer a §6.3 slot the fast path does not. Pinned by
`an_intercepted_intro_leaves_no_stage_zero_entry` (both key orders).

## Mutations, executed
| Mutation | Reds |
|---|---|
| no interception at all | `read_identity_intercepts_a_parked_intro_whose_claim_is_a_pending_remote`, `the_losers_msg2_anchors_at_the_msg1_source_not_the_dialled_address`, `an_intercepted_intro_leaves_no_stage_zero_entry` |
| interception hoisted above ruling 74's early return (unconditional on chain state) | `a_second_read_identity_after_the_static_became_pending_does_not_intercept` |

The second is the test author's, and it is the one that pins §6.4:1439 — the
premise §6.4's whole PENDING branch rests on.

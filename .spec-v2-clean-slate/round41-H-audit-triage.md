# H — Triage of the 2026-08-17 audit residue (items 6–8)

Base: `94dab20` (verified, clean tree). Audit text recorded at `6448e6f`,
before rulings 249–257 landed.

## 0. Base verification

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-a8dacabb5cdb62177
$ git rev-parse HEAD
94dab200a172a5b62b4d28711962574d61a9d3c8
$ git status --short
(clean)
$ git log --oneline -3
94dab20 Ruling 257: the bare FIN defers when no frame fits
321618c Rulings 256 and 257: the round-41 ranked pair, ruled on measurement
1395ea6 Round 40 closes: the round-41 material, consolidated
```

## 1. Audit item 6 — coverage

Note on the numbering: `O13`/`O43e`/`O53a`/`O53b` are the **audit's own**
ids, defined in `audit/G-obligations-trend.md`. They do not appear in
`SPEC.md`; the obligations themselves live in Appendix B (from
`SPEC.md:7090`). Each was re-verified against the suite at HEAD rather
than taken from the audit.

### 1.1 Appendix B NONE: O13 — the post-mortem pin's LRU-flush half

**STILL NONE at HEAD.** Obligation (§17.1, ruling 37), `SPEC.md:7178-7183`:

> The post-mortem pin (§17.1, ruling 37): an entry written by a tie-break
> admission or a winner-side record survives orphan aging **and** a full
> LRU flush (≈ 1024 authenticate-then-drop statics) for `HANDSHAKE_GIVEUP`
> after its connection dies… the test must pin **both** sides of the
> horizon.

```
$ grep -rn "ruling 37\b" src/ tests/
(no hits)
```

The one candidate test,
`losing_the_internal_tie_break_pins_the_record_against_orphan_aging`
(`src/core/endpoint/tests.rs:1334`), drives only `TS_GUARD_ORPHAN_TTL`
(15 s, live connection) — no teardown, no 1024-entry flush, no
`HANDSHAKE_GIVEUP` (90 s) horizon. The file carries its own standing gap
note at `src/core/endpoint/tests.rs:2212-2225` saying exactly that, and
it is unchanged. `TS_GUARD_ORPHAN_CAP` (1024, `src/constants.rs:478`) is
exercised **behaviourally nowhere** — only its definition, its use at
`guard.rs:486`, and its value pin in `tests/spec_constants.rs`. (The
"1024" eviction tests in `src/core/tests.rs` are `INTRO_QUEUE_CAP`, §6.3's
staged queue — a different cap.)

**Classification: STILL OPEN — TEST GAP.**
**Separating assertion:** authenticate-then-drop ~1024 distinct statics
to force a full LRU flush of the guard tier, then replay the original
captured initiation **inside** the 90 s horizon and assert it is still
rejected at the guard (not surfaced as a fresh intro); replay it again
**after** 90 s and assert it is admitted. This separates three ways: a
build with no post-mortem pin passes the after-90 s half for free but
fails "still rejected despite the flush"; a build with an immortal pin
fails the after-90 s half. Only a test that actually drives the
1024-entry flush distinguishes them — which is why the existing
orphan-aging test cannot be extended, it must be joined.

### 1.2 Appendix B NONE: O43e — per-session amplification budget independence

**STILL NONE at HEAD.** Obligation (§7.3, ruling 170), `SPEC.md:7517-7519`:

> Per session (ruling 170). Two connections anchored to the same peer
> address hold two independent budgets: exhausting one must not throttle
> the other, and funding one must not credit the other.

```
$ grep -rn "ruling 170" . --include="*.rs"       → 0 hits in any test file
$ grep -rn "independent budget|two connections|same peer address" src/ tests/  → 0
```

`ruling 170` appears only in **doc comments** —
`src/core/connection/mobility.rs:25,74-77` and
`src/core/connection/mod.rs:233` — where mobility.rs:74-77 states the
design residual in prose ("N sessions to one address multiply the
reflector by N") without asserting it. Every budget test in
`tests_path.rs` (57 tests), `tests_roam.rs`, `tests_contested.rs` and
`tests/story_path.rs` uses a **single-connection** harness. This is the
one obligation in the set with literally zero coverage of any kind.

**Classification: STILL OPEN — TEST GAP.**
**Separating assertion:** stand up two connections anchored to the *same*
peer address; drive connection A's `sent` to its 3× cap against an
unvalidated address, then assert connection B can still transmit up to
its **own** full 3× cap. A build that keys the amplification budget by
peer **address** rather than by session — the natural implementation slip
and the one that matters, since it is a security counter — throttles B
and fails. The symmetric half: fund A's `recv` counter and assert B's cap
is unmoved, which catches a shared budget letting B spend credit it never
earned.

### 1.3 Appendix B NONE: O53a / O53b — and a live conflict about their status

Both **STILL NONE at HEAD**, both under `SPEC.md:7651`'s heading
*"Post-implementation validation obligations (gates on the flagged
rulings)."*

**O53a** — the ACK-loss-burst simulation (§7.2/D-5 gate),
`SPEC.md:7653-7658`: FlakyWire on the paused clock, sustained ACK-loss
bursts against the 2048-bit fused window under the every-2nd-ACK policy,
*"quantifying spurious-retransmit and false-congestion-event rates"*,
with §19's range-tracker ACK named as the ready remedy *"before
ratification hardens the fused choice"*.

```
$ grep -rln "spurious.retransmit|false.congestion|ACK.loss.burst" --include="*.rs" .
(0 hits)
```

Nothing anywhere asserts a **rate**. Ordinary single-loss unit tests in
`tests_recovery.rs`/`tests_ack.rs` pass regardless of burst behaviour.

**O53b** — the window-constants throughput sanity check (§10.2/§10.6
gate), `SPEC.md:7659-7663`: bulk transfer *"within 20 % of quinn under
its shipped defaults"*, no stall, with the §10.6 reassembly bound active,
*"before the §10.2 constants and `REASSEMBLY_CHUNKS_MAX` ratify"*.

```
$ grep -rn "quinn" --include="*.toml" --include="*.rs" .
→ two doc-comment mentions only (recovery.rs:64, shared.rs:48); no dependency
```

`benches/throughput.rs` exists and is actively used (ruling 247's
measurement; ruling 253 calls it *"the instrument"*), but it **prints**
absolute MiB/s — no quinn baseline, no 20 % assertion, nothing that can
go red.

**⚠ CONFLICT — reported, not resolved (working rule 3).** These two
obligations are stated as gates that run **before** ratification
decisions harden. `SPEC.md` was ratified 2026/08/14; the constants they
were meant to validate (§10.2, `REASSEMBLY_CHUNKS_MAX`, the fused window)
are already ratified and wire-frozen. So:

- As **literal pre-ratification blocking gates** they are moot — running
  them now cannot block a decision already taken, and CLAUDE.md forbids
  moving a ratified constant on their evidence without a fresh ruling
  anyway.
- As **stated Appendix B obligations** they are live — nothing in
  `SPEC.md` or `rulings.md` (249-257 checked, the only rulings since the
  audit) marks either satisfied, waived, or historical. They stand
  word-for-word at HEAD.

I am not picking a side. **Classification: STILL OPEN — NEEDS RULING
(status).** **The decision:** for each of O53a and O53b, either (a) mark
it discharged-as-moot in Appendix B with a one-line ruling recording that
ratification overtook it; (b) keep it live and re-scope it from a *gate*
to a *regression bound* (O53b in particular becomes useful as "throughput
does not regress >X % against a pinned local baseline" — no quinn
dependency, and `benches/throughput.rs` is already the instrument); or
(c) run them as stated. Note (c) for O53b means taking a `quinn`
dev-dependency, which is a supply-chain decision (`cargo deny`) as much
as a test one. **The audit ranked this its #1 finding, and that ranking
still holds** — it is the only item in this report where the *spec's own
text* is in an unresolved state.

### 1.4 guard.rs 4-site `pins` cluster below the LRU cap

**STILL NONE — and it is the same hole as O13, not a second one.** The
audit itself flagged them as "same area, two methods"
(`audit/AUDIT.md:224`), and the re-verification confirms one shared root
cause: **no test ever drives the guard table's LRU tier.**

The four sites, at HEAD:

```
$ grep -n "fn pinned\|fn age_deadline\|fn revert\|fn unpin\|TS_GUARD_ORPHAN_CAP" src/core/endpoint/guard.rs
165:    fn pinned(&self, now: Instant) -> bool {
190:    fn age_deadline(&self) -> Option<Instant> {
300:    pub(crate) fn revert(&mut self, undo: GuardUndo) {
392:    pub(crate) fn unpin(&mut self, key: &[u8], kind: PinKind, now: Instant) {
486:            > constants::TS_GUARD_ORPHAN_CAP
```

`audit/A-mutants.md:273-282,332-343` records mutations missed at all four
`pins`-boundary checks — "three independent misses on the" same boundary,
plus a fourth. The reason they are all missed is `guard.rs:486`: the
eviction path gated on `TS_GUARD_ORPHAN_CAP` is never entered by any
test, so the pin-vs-evict interaction those four boundaries exist to
arbitrate is never exercised at all. Mutating any of them changes
behaviour only in a region the suite does not visit.

**Classification: STILL OPEN — TEST GAP (merge with 1.1).**
**Separating assertion:** the O13 test in 1.1 discharges this too, if it
asserts on the pinned entry's *survival across the flush* rather than
only on the final admit/reject verdict — i.e. after driving >1024
authenticate-then-drop statics, assert the pinned entry is **still
present in the table** while unpinned contemporaries have been evicted. A
build where `pinned()` returns `false` at the boundary evicts it and
fails; the current suite cannot tell the two apart because it never fills
the table. **Recommendation: write 1.1 and 1.4 as one test, and count
them as one item of work.**

### 1.5 NewReno::reset on roam — **MEASURED: unfalsifiable, and I proved it**

**CONFIRMED at HEAD, by experiment rather than by reading.** This is the
strongest finding in the report.

A test with the right name exists —
`a_roam_resets_the_controller_and_keeps_the_flight`,
`src/core/connection/tests_roam.rs:734` — and asserts
(`tests_roam.rs:748-752`):

```rust
    assert_eq!(
        solo.conn.congestion_window(),
        constants::INITIAL_WINDOW,
        "§14.6: cwnd resets to INITIAL_WINDOW"
    );
```

**But the fixture never grows `cwnd` above `INITIAL_WINDOW` before the
roam.** It writes 800 bytes, flushes, drains, and roams — processing no
ACKs, so slow start never runs. And `NewReno::new()` already sets
`cwnd = INITIAL_WINDOW` (`src/core/connection/congestion.rs:62-64`),
which is the identical value `reset()` writes (`:83`). The assertion
therefore compares the post-state against a value the pre-state already
held. Working rule 9 exactly: *a bound is only a test if the degenerate
case violates it* — and **a name is not a pin**.

**Verified by measurement, not inference.** I stubbed `reset()` to a
no-op (`let _ = now;`), leaving the rest of the crate untouched:

```
$ cargo test --lib a_roam_resets_the_controller_and_keeps_the_flight
running 1 test
test core::connection::tests_roam::a_roam_resets_the_controller_and_keeps_the_flight ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 743 filtered out
```

The test that names the property passes with the property deleted. Then
the whole suite:

```
$ cargo test --all-features 2>&1 | grep -E "^test result"
test result: ok. 744 passed; 0 failed; ...   (lib)
test result: ok. 24 passed; ...
... 22 test binaries, all ok ...
$ # summed: 1069 passed, 0 failed, 1 ignored
```

**1069 tests, zero failures, with §14.6's roam congestion reset entirely
removed.** `cwnd`, `ssthresh`, `recovery_start` and `acked_accum` all
stop being reset on roam and nothing anywhere goes red.

Stub reverted; tree clean:

```
$ git checkout -- src/core/connection/congestion.rs && git status --short
REVERTED-CLEAN
```

**Classification: STILL OPEN — TEST GAP (highest value in this report).**
**Separating assertion:** before the roam, drive ACKs until
`congestion_window() > INITIAL_WINDOW` and **assert that precondition**
(so the test fails loudly if the fixture ever stops growing the window);
then roam; then assert `congestion_window() == INITIAL_WINDOW`. The
stubbed build fails at the third step. Two cheap companions, since the
accessors already exist under `#[cfg(test)]`
(`congestion.rs:99-108`): drive a congestion event to pull `ssthresh`
below `u64::MAX`, roam, assert `ssthresh() == u64::MAX`; and assert
`recovery_start() == Some(roam_instant)` — the last one is the only pin
for the "**set to the roam instant, not cleared**" clause that
`congestion.rs:71-81`'s doc calls out as the subtle half of ruling 137.

### 1.6 authenticate() idempotency

**STILL NONE at HEAD.** The `Proven` arm that makes a second
`authenticate()` call return the same answer is
`src/core/endpoint/staged.rs:463-468`:

```rust
            match &entry.state {
                ChainState::Proven {
                    peer, timestamp, ..
                } => {
                    return Ok((peer.clone(), *timestamp));
                }
                ChainState::Claimed { .. } => {}
                _ => return Err(AuthError::Expired),
            }
```

Delete that arm and a second call falls through to
`_ => return Err(AuthError::Expired)`. `audit/AUDIT.md:22` reports the
mutation as unobserved.

**A near-miss worth naming, because it is what makes this easy to
mis-clear.** `src/core/endpoint/tests.rs:881-885` *does* test
idempotency, and cites ruling 74 for it — but of **`read_identity()`**,
not `authenticate()`:

```rust
    let second = local
        .ep
        .read_identity(now, id)
        .expect("ruling 74: idempotent, and §6.4:1439 says the interception cannot fire here");
```

Different verb, different state arm (`read_identity` is answered from
`ChainState::Claimed`/`Proven` at `staged.rs:269-272`). A grep for
"idempotent" near the staged verbs finds this and looks like coverage.
It is not.

**Classification: STILL OPEN — TEST GAP.**
**Separating assertion:** call `authenticate()` twice on the same
`IntroId` and assert the second call returns `Ok` with the **same peer
static and the same timestamp** as the first, **at zero incremental DH**
(`local.dh()` unchanged — the fixture already has the counter, used at
`tests.rs:887`). A build with the `Proven` arm deleted returns
`Err(AuthError::Expired)` on the second call and fails; the DH-count half
additionally catches a build that "succeeds" by redoing the work, which
is the failure mode a bare `Ok`-check would wave through.

### 1.7 F2 unfalsifiable per-source-cap assertion

**CONFIRMED unchanged at HEAD, with a one-token fix already verified by
the audit.**

`the_per_source_cap_is_four_chains_per_ip` (`src/core/tests.rs:1117`)
contains the assertion that names the cap's whole security property:

```rust
    assert!(b.present(other), "the cap reached across source IPs");
```

`audit/F-test-strength.md:213-245` showed it **passes under mutation
M4c** — `intro_queue.rs:248`'s
`self.oldest_unconsumed(Some(source_key))` → `oldest_unconsumed(None)`,
i.e. one source's flood evicting a *different* source's parked chain,
which is precisely the DoS the per-source cap exists to prevent. Only one
unrelated test caught it.

**Why it cannot fail, verified unchanged at HEAD.** The fixture parks the
four same-source entries at `t+0s … t+3s`
(`src/core/tests.rs:1121-1122`: `let now = t + Duration::from_secs(n as
u64);`) and parks `other` at `t` (`src/core/tests.rs:1141-1142`:
`b.feed(t, v4(6, 100), …)`). So `other` and `ids[0]` share an `age_key`,
`oldest_unconsumed` breaks the tie by `IntroId` (`intro_queue.rs:385`),
and `ids[0]` was inserted first — the global oldest and the per-source
oldest are **the same entry**. Correct and mutated builds evict
identically.

**Classification: STILL OPEN — TEST GAP (trivial fix, high value).**
**Separating assertion:** the audit already found and *verified* it —
change `Duration::from_secs(n as u64)` to `Duration::from_secs(n as u64 +
1)` at `src/core/tests.rs:1122`, making `other` strictly the globally
oldest entry. F re-ran M4c against the changed fixture and the test then
failed at `src/core/tests.rs:1152`. One token. **One caution when
applying it:** the "fifth" arrival is fed at
`t + INTRO_MAX_PER_SOURCE` seconds (`src/core/tests.rs:1147`), which
after the shift ties with `ids[3]`; harmless for the eviction under test
(the victim is `ids[0]` either way), but bump it to `+ 1` as well to keep
the fixture's ordering strict rather than incidentally-correct — the
exact species of accident this finding is about.

## 2. Audit item 7 — hardening
### 2.1 spin detector is debug_assert!

**RESOLVED — by rulings 249(ii) + 255.** Verified at HEAD.

```
$ grep -n "spin" src/shell/driver.rs
...
963:    /// # The spin detector, and why it is not `deadline >= now` (**F1**)
...
```

`src/shell/driver.rs:978-979` now carries the ratified heading:

> `/// # It is an `assert!`, not a `debug_assert!` (**[RATIFIED 2026/08/17`
> `/// — ruling 249(ii)]**)`

and the live code at `src/shell/driver.rs:1074` is:

```rust
                if let Some(fired_at) = self.last_timeout {
                    if *announced <= fired_at {
                        let streak = self.overdue_streak.get() + 1;
                        self.overdue_streak.set(streak);
                        assert!(streak < 3, "{PAST_DEADLINE}");
                    } else {
                        self.overdue_streak.set(0);
                    }
                }
```

A release-mode `assert!`, so the class is no longer invisible in the
release gate. Ruling 255 amended 249(ii) to trip on the **third
consecutive** overdue announce rather than the first, because a benign
one-step overdue (a `Loss` firing that retransmits nothing behind an ACK
that already shrank the `Pto` deadline ~4 ms into the past) is the
"overdue is not spinning" case. Threshold, not mechanism.

**Classification: RESOLVED.** No maintainer decision needed.

#### 2.1b — adjacent finding, NOT the audit's item (new)

The *sibling* assertions in the same function are still `debug_assert!`,
and their release behaviour is a **silent data loss**, not a spin:

`src/shell/driver.rs:1040-1053`

```rust
        let endpoint = match self.shell.state.borrow_mut().endpoint.poll_output() {
            EndpointOutput::Timeout(deadline) => deadline,
            _ => {
                debug_assert!(false, "the endpoint core queued an output outside a drain");
                None
            }
        };
        ...
                    _ => {
                        debug_assert!(false, "a connection core queued an output outside a drain");
                        None
                    }
```

The function's own doc-comment, `src/shell/driver.rs:1026-1031`, states
the consequence explicitly:

> `/// `poll_output()` **pops**, so the `_` arms below do not merely`
> `/// mis-report a deadline — they *destroy* whatever the core queued,`
> `/// which for a connection core is a datagram.`

So in release these arms drop a queued datagram and continue, with
nothing red anywhere — the same "silent in release" shape ruling 249(ii)
promoted the spin detector for, applied to a *different* failure. This
was not the audit's item 7 claim and is offered as a **new** hardening
candidate, not a re-verification. Note it is genuinely harder than the
spin case: 249(ii)'s promotion was safe only because ruling 249(i) had
first removed the one protocol-reachable trip; nobody has done the
equivalent reachability argument for these two arms, and the fix may be
"return the deadline you popped and re-queue the output" rather than a
promotion at all.

### 2.2 recv.rs:526 bare `+` vs §8.4

**The line moved: `recv.rs:526` at `6448e6f` is `recv.rs:750` at HEAD.**
Same expression, same function (`Reassembly::insert`), unchanged.

```
$ git show 6448e6f:src/core/connection/recv.rs | sed -n '526p'
        let end = offset + data.len() as u64;
$ sed -n '750p' src/core/connection/recv.rs
        let end = offset + data.len() as u64;
```

**The §8.4 mandate is real.** `SPEC.md:3457-3458`, in the STREAM frame
block:

> All offset arithmetic (`offset + length`,
> final-size and credit comparisons) is checked or saturating.

and the structural rule it pairs with, `SPEC.md:3450-3451`:

> Structural errors: `length` overrunning the
> plaintext; a ¬LEN frame that is not final; `offset + length` exceeding
> 2⁶² − 1.

**VERDICT: the overflow is NOT reachable. Three independent bounds, and
the first one alone is sufficient.**

*Bound 1 — parse time, and this one is decisive.* Every STREAM frame is
decoded by `Stream::parse_body`, `src/core/connection/frame.rs:412-416`:

```rust
        let end = offset
            .checked_add(data.len() as u64)
            .filter(|end| *end <= VarInt::MAX_VALUE)
            .ok_or(Structural::StreamOffsetOverflow)?;
```

with `VarInt::MAX_VALUE = (1u64 << 62) - 1` (`src/varint.rs:41`). So any
frame reaching the receive path already satisfies
`offset + data.len() ≤ 2⁶² − 1 ≈ 4.61e18`, against `u64::MAX ≈ 1.84e19`.
The bare `+` at `recv.rs:750` recomputes **the same expression** on the
same two operands. It cannot overflow: its result is proven ≤ 2⁶² − 1
before the value ever leaves the parser. Even absent the `filter`, the
`offset` operand is a varint and thus ≤ 2⁶² − 1 by construction, and
`data.len()` is bounded by the packet plaintext — the sum has ~2 spare
bits regardless.

*Bound 2 — the semantic check, immediately upstream.*
`src/core/connection/streams.rs:733` calls `recv.check_stream(...)`,
which opens (`src/core/connection/recv.rs:227`) with

```rust
        let end = offset.checked_add(len).ok_or(Violation::FinalSize)?;
```

and `apply_stream` — the *only* non-test caller of `insert`
(`recv.rs:282`) — is invoked five lines later at `streams.rs:738`, with
nothing between them that can mutate `f.offset` or `f.data`.

*Bound 3 — the credit ceiling.* `check_stream` also requires
`max(high_water, end) ≤ self.credit.advertised()` (`recv.rs:239-242`);
`advertised()` returns `last_advertised` (`flow.rs:109-111`), raised only
to a `prospective` derived from locally-`consume`d bytes
(`flow.rs:142-143`), so reaching 2⁶² would require actually delivering
~4.6 exabytes.

*Also note the local inconsistency the audit is really pointing at:*
`apply_stream` computes **this exact quantity** two lines earlier with
`offset.saturating_add(data.len() as u64)` (`recv.rs:274`) and then
`insert` recomputes it bare. One expression, two arithmetic disciplines,
seven lines apart.

**Classification: STILL OPEN — CODE HARDENING (cosmetic; provably
bounded).** Not a live defect and not a ruling: the value is provably
identical under any of `+`, `checked_add().unwrap()` or
`saturating_add`, so no wire byte and no observable behaviour moves.
Fix: `let end = offset.saturating_add(data.len() as u64);` at
`recv.rs:750`, matching `recv.rs:274`, with a comment citing
`frame.rs:412-416` as the invariant's real home.

#### 2.2b — the bound that does the work is UNTESTED (new, and the better finding)

The bare `+` is safe *because of* `frame.rs:412-416`. That guard has no
test:

```
$ grep -rn "StreamOffsetOverflow" src/
src/core/connection/frame.rs:416:            .ok_or(Structural::StreamOffsetOverflow)?;
src/core/connection/frame.rs:678:    StreamOffsetOverflow,
$ grep -rn "StreamOffsetOverflow\|offset_overflow" tests/ src/core/connection/tests.rs src/core/tests.rs
(no output)
```

Constructed at one site, declared at one site, asserted nowhere. So the
only §8.4 structural error in the STREAM block that is *not* exercised is
the one every downstream `u64` sum silently depends on.

**Separating assertion (working rule 9):** a STREAM frame carrying
`offset = 2⁶² − 1` with **one** byte of data must be rejected as a
structural violation, not accepted. This separates properly: `2⁶² − 1 +
1` does **not** overflow `u64`, so a degenerate build that keeps
`checked_add` but drops the `.filter(|end| *end <= VarInt::MAX_VALUE)`
clause returns `Ok` and passes any test written against overflow alone.
The `filter` is the load-bearing half, and because the varint decoder
already caps `offset` at `2⁶² − 1`, a value that trips `checked_add` is
**unconstructible from the wire** — the filter is the only reachable
failure mode, and any test that does not sit right at the ceiling asserts
nothing. **Classification: STILL OPEN — TEST GAP.**

### 2.3 IntroError::Expired conflates per-IP-cap eviction with TTL

**CONFIRMED at HEAD, and it is worse than "conflates" — the `Display`
string is affirmatively false in the eviction cases.**

`src/error.rs:99-102`:

```rust
pub enum IntroError {
    /// The parked introduction outlived `INTRO_TTL`.
    #[error("the parked introduction outlived INTRO_TTL")]
    Expired,
```

**One production site, three causes.** `Expired` is produced by a table
miss, `src/core/endpoint/staged.rs:267`:

```rust
            let entry = self.intros.get(id).ok_or(IntroError::Expired)?;
```

An `IntroId` is absent from `self.intros` for **three** different
reasons, all of which land on this one line:

1. TTL expiry — the entry outlived `INTRO_TTL` (15 s). The only cause the
   message describes.
2. **Per-source-cap eviction** — `src/core/endpoint/intro_queue.rs:247-250`:
   `if u64::from(self.count_for(source_key)) >= self.max_per_source ...
   Some(victim) => evicted = self.remove(victim)`.
3. **Global-cap eviction** — `intro_queue.rs:259-262`, the same shape
   against `self.cap` (`INTRO_QUEUE_CAP`, 1024).

(The second `Expired` at `staged.rs:275` is the `ChainState::Poisoned`
arm, guarded by `debug_assert!(false, "Poisoned is never observable")` —
a fourth cause, but a defensive one.)

**What an operator sees today.** For all three: the string *"the parked
introduction outlived INTRO_TTL"*. In cases 2 and 3 that is not a vague
error, it is a **wrong** one — it reports a 15-second timeout for an
event that happened in microseconds under queue pressure, which points
an operator at latency and TTL tuning when the actual signal is *"you
are at your intro-queue cap"* — the single most useful thing to know
during a flood, and the one thing the error hides.

**No trace compensates for it.** Both eviction call sites discard
everything except the guard state — `src/core/endpoint/mod.rs:653-659`
and `src/core/endpoint/routing.rs:275-277` both do only
`self.release_chain_guard_state(now, evicted.guard_undo,
evicted.guard_pin);` and drop `evicted` on the floor. No counter, no
event, no §18.2 row. So the eviction is invisible **twice**: mislabelled
in the error and unrecorded in the trace.

**What an operator would need to tell them apart:** either a distinct
variant (`Evicted`, or `Evicted { per_source: bool }`), or — cheaper and
non-breaking — an §18.2 trace/counter emitted at the two eviction sites,
which already hold the `IntroEntry` and its `SourceKey`.

**API-visibility: YES, it is `pub`.** `pub enum IntroError` at
`src/error.rs:99`, re-exported from the crate root at `src/lib.rs:272`.
`IntroError` is `#[derive(PartialEq, Eq)]` and the crate's own tests
match it exhaustively (`src/core/tests.rs:1675`,
`src/core/endpoint/tests.rs:826`), so **adding a variant is a
breaking change** for any downstream `match` without a wildcard arm.

**Classification: STILL OPEN — NEEDS RULING (API surface).**
**The decision:** do we (a) split the variant — semantically right,
breaking, and the enum is pre-1.0 so the cost is now-or-never; (b) add
the §18.2 eviction trace and leave the variant alone — non-breaking,
recovers the operator signal, leaves the false `Display` string in
place; or (c) keep (b) *and* reword the `#[error(...)]` string to
something true of all three causes ("the parked introduction is no
longer queued"), which is non-breaking and removes the active
misinformation. **(c) looks like the cheap correct answer** and needs
only a ruling on the wording, but the call is the maintainer's.
Note none of this moves a wire byte — `IntroError` is a local API type,
not a wire code.

### 2.4 per-IP cap of 4 behind NAT

**LARGELY RESOLVED — this is documented, deliberate, and configurable.**

The code states the intent directly, `src/core/endpoint/intro_queue.rs:9-15`:

> `//! # Two keys, two scopes, on purpose`
> `//!`
> `//! The **dedup** key is the full [SocketAddr]; the **cap** key is the IP`
> `//! (a /64 for IPv6). They differ deliberately: distinct initiators behind`
> `//! one NAT present distinct ports, so they dedup separately while sharing`
> `//! one cap, which is the stated intent.`

And the constant is **tunable by the operator who has the NAT problem**:

```
$ grep -n "INTRO_MAX_PER_SOURCE" src/constants.rs
472:pub const INTRO_MAX_PER_SOURCE: usize = 4;
653:const _: () = assert!(INTRO_MAX_PER_SOURCE <= INTRO_QUEUE_CAP);
```

with `Config::with_intro_max_per_source` at `src/config.rs:162-164` and
the accessor at `:201`. `SPEC.md:1270` marks it **`configurable`** in
§6.3's own constant table, so raising it needs no ruling at all.

Two real mitigations beyond the knob, both from `SPEC.md:1299-1311`:
eviction "operates on the unconsumed tier only" and "consumed chains are
DH-paid, and non-evictable for it", so a client that has reached
`read_identity()` cannot be evicted by a NAT peer; and age is by **last
refresh** (ruling 69), so an honest retransmitting initiator keeps making
itself young.

**One genuine residue, small.** The code's phrase *"which is the stated
intent"* attributes to the spec slightly more than the spec says. §6.3's
NAT sentence lives in the **dedup** bullet and covers only dedup —
`SPEC.md:1293-1295`:

> `Distinct initiators behind one NAT present distinct ports, hence distinct`
> `keys; a same-4-tuple collision is a rebind of the same flow, for which`
> `newest-wins is correct.`

The spec states the cap **key** is the source IP (`SPEC.md:1270`,
`:1299`), so NAT-sharing follows as a direct consequence — but the
consequence is never *stated*, and it is not stated in the one bullet
where §6.3 does discuss NAT. That is working rule 8's shape (a stated
construction whose scope is left unstated) in its mild form: not a
contradiction, a documentation gap sitting next to the place a reader
would look for it.

**Classification: STILL OPEN — NEEDS RULING (spec prose only; trivial).**
The decision: add one clause to §6.3's per-source-cap bullet stating that
initiators sharing a public IP share one allowance, and naming
`with_intro_max_per_source` as the operator's remedy. No constant moves,
no wire byte moves. Lowest-value item in this report — list it, do not
prioritise it.

## 3. Audit item 8 — unexplained
### 3.1 datagram p99 22 ms on real loopback at 0 % CPU

**Triaged from existing evidence (agent D's and agent C's own files).
Verdict: the "sequential `send_to`" hypothesis is architecturally real
but quantitatively implausible; and one leg of the anomaly's premise does
not survive re-reading.**

**What D actually measured** (`audit/D-real-socket.md:149,180,205`): one
process, a **single OS thread carrying both endpoints** (`:159`), phase C
= 1000 *sequential, unpipelined* protocol round trips over real loopback
UDP on a real clock.

```
PHASE_C_OK n=1000 p50_us=131 p99_us=22105 min_us=41 max_us=86094   (release)
PHASE_C_OK n=1000 p50_us=510 p99_us=63424 min_us=164 max_us=195053 (debug)
```

D's own reading of the debug run (`:244-247`): the tail is *worse* in
debug, which "supports the Run 1 hypothesis that the tail is
scheduling/executor overhead" — a slower build makes the same kind of
pause larger, not different in kind.

**The premise correction — "at 0 % CPU" is not supported by the samples
that were taken.** `audit/cpu-samples.log` is four `ps` lines, all `0.0`,
taken at elapsed 25 s / 35 s / 45 s / 55 s; `sample_cpu.sh` and
`D-real-socket.md:165` both label the window explicitly *"CPU sampling
during idle"* — the 60 s idle phase that begins **after** phases A/B/C
have finished, in under ~2 s. **No CPU sample was taken concurrently with
phase C.** D's summary sentence at `:198`/`:374` ("CPU stayed at 0.0 %
throughout the whole run") overstates its own instrument. This is working
rule 12 in miniature: a true measurement (the endpoint idles at 0 % CPU)
imported into an argument about a different state (the loaded phase).
It does not refute the anomaly — a 10 s-granularity sampler could not
resolve a sub-second phase either way — but "22 ms **at 0 % CPU**" should
stop being repeated as a measured fact.

**What C measured, and why it does not settle it.** C's E8
(`audit/C-liveness.md:333-349`) was added mid-task specifically to test
D's number, and found handle→wire and wire→parked-recv both **0 virtual
ns** over 760 round trips at three cadences, `nonzero=0` at every
percentile — on `FlakyWire` and a **paused clock**. C says so itself
(`:456-457`): *"The real 22.1 ms p99. Not reproducible on the paused
clock."* That eliminates a protocol-internal cause (no core or driver
timer inserts the delay) and specifically falsifies the `MAX_ACK_DELAY`
= 25 ms coincidence. It cannot speak to real syscall or scheduler cost,
which the paused clock does not model. **C and D do not measure the same
quantity**, so C is an elimination, not an answer.

**The `send_to` hypothesis, checked against the code.**
`src/shell/driver.rs:891-902` confirms the shape — one `await` per
datagram, no batching (`sendmmsg`/GSO appear nowhere in `src/`), over a
plain `tokio::net::UdpSocket::send_to` (`src/shell/wire.rs:77-78`):

```rust
    async fn transmit(&mut self, outgoing: Vec<Outgoing>) {
        for Outgoing { conn, transmit } in outgoing {
            let Transmit { to, data } = transmit;
            if let Err(error) = self.wire.send_to(&data, to).await { ... }
```

**But it cannot be this.** Phase C is ping-pong with one request
outstanding, so each drain yields ~one `Outgoing` — a loop of length 1
has no serialisation to expose — and loopback `send_to` costs single-digit
microseconds, three to four orders of magnitude short of 22 ms.
Sequential `send_to` is a genuine **throughput/fan-out** concern (many
connections draining in one pass), not a single-item latency one.

**Leading hypothesis instead: real OS/tokio scheduler jitter.** Both
endpoints share one OS thread (`D:159`); when that thread is not
scheduled, both drivers stall together, which presents as low CPU
(blocked, not spinning) and scales with build slowness exactly as `:244`
observed. Pacing is ruled out by construction —
`src/core/connection/congestion.rs:8-10` cites §14.7: *"no pacing … no
sub-RTT wakeups."*

**Classification: INVESTIGATE-LATER (not a defect; premise partly
unsupported).** **The one named next measurement:** split D's existing
phase-C timer in `examples/audit_udp.rs` into two series — `t1−t0`
(handle-send call returns) and `t2−t1` (reply observed) — and rerun the
same 1000 iterations (sub-second, no new dependencies, no new harness).
p99 of `t1−t0` ≈ 22 ms indicts the send path; p99 of `t2−t1` ≈ 22 ms with
`t1−t0` in microseconds indicts wake/scheduling and closes the item as
"not slither". Note the harness is **uncommitted scratch** — it must be
recreated or recovered before this can run.

## 4. TRIAGE TABLE

| # | Sub-claim | Classification | Evidence | Decision? | Next step |
|---|---|---|---|---|---|
| 1.1 | O13 — post-mortem pin survives a full LRU flush | **OPEN — TEST GAP** | `grep "ruling 37" src/ tests/` → 0; gap self-noted `endpoint/tests.rs:2212-2225`; `TS_GUARD_ORPHAN_CAP` never driven | n | Write with 1.4 as one test: flush >1024 statics, assert pinned entry rejected inside 90 s and admitted after |
| 1.2 | O43e — two sessions, one peer addr, independent budgets | **OPEN — TEST GAP** | `grep "ruling 170" --include=*.rs` → 0 test hits; all budget tests single-`Solo` | n | Two connections to one address; exhaust A's 3× cap, assert B's cap intact; then fund A, assert B uncredited |
| 1.3 | O53a/O53b — Appendix B pre-ratification gates | **OPEN — NEEDS RULING (status)** | `SPEC.md:7651,7653-7663`; no quinn dep, no rate assertion; rulings 249-257 silent | **y** | Rule per gate: discharge-as-moot, re-scope to a regression bound, or run as stated (O53b implies a `quinn` dev-dep) |
| 1.4 | guard.rs 4-site `pins` boundary cluster | **OPEN — TEST GAP (dup of 1.1)** | `guard.rs:165,190,300,392`; eviction at `:486` never entered; `A-mutants.md:273-282,332-343` | n | Fold into 1.1; add "pinned entry still present after the flush" to that test |
| 1.5 | `NewReno::reset` on roam unobserved | **OPEN — TEST GAP (top priority)** | **Measured:** stubbed `reset()` → **1069 tests, 0 failures**; assertion compares against the value `new()` already set | n | Grow cwnd via ACKs + assert precondition, roam, assert reset; add `ssthresh`/`recovery_start` pins |
| 1.6 | `authenticate()` idempotency unobserved | **OPEN — TEST GAP** | `staged.rs:463-468` `Proven` arm; the ruling-74 idempotency test at `endpoint/tests.rs:885` is **`read_identity`**, a different verb | n | Call `authenticate()` twice: same peer + same timestamp, **and zero incremental DH** |
| 1.7 | F2 unfalsifiable per-source-cap assertion | **OPEN — TEST GAP (one token)** | `core/tests.rs:1122` + `:1142` tie on `age_key`; `F-test-strength.md:213-245` verified the fix | n | `from_secs(n as u64)` → `+ 1` at `core/tests.rs:1122`; bump `:1147` too |
| 2.1 | Spin detector is `debug_assert!` | **RESOLVED** — rulings 249(ii)+255 | `driver.rs:978-979` ratified heading; live `assert!(streak < 3, …)` at `driver.rs:1074` | n | None |
| 2.1b | *(new)* the two sibling `debug_assert!(false)` arms silently **destroy** a queued datagram in release | **OPEN — CODE HARDENING** | `driver.rs:1040-1053`; consequence stated in its own doc at `:1026-1031` | **y** | Decide: reachability argument then promote, or re-queue instead of dropping. Harder than 249(ii) — no equivalent of 249(i) has been done |
| 2.2 | `recv.rs:526` bare `+` vs §8.4 | **OPEN — CODE HARDENING (cosmetic)** | Now `recv.rs:750`. **Not reachable**: `frame.rs:412-416` caps `offset+len ≤ 2⁶²−1` at parse; `check_stream` `checked_add` at `recv.rs:227`; credit ceiling | n | `saturating_add`, matching `recv.rs:274` which computes the same value seven lines earlier |
| 2.2b | *(new)* the guard that makes 2.2 safe is itself untested | **OPEN — TEST GAP** | `StreamOffsetOverflow` constructed at `frame.rs:416`, asserted **nowhere** | n | STREAM frame at `offset = 2⁶²−1` + 1 byte must be a structural error — trips the `filter`, which `checked_add` alone cannot catch |
| 2.3 | `IntroError::Expired` conflates eviction with TTL | **OPEN — NEEDS RULING (API surface)** | One site `staged.rs:267`, **three** causes (TTL, per-source evict `intro_queue.rs:247-250`, global evict `:259-262`); no trace at either evict site; `pub` at `error.rs:99`, re-exported `lib.rs:272` | **y** | Recommend (c): add the §18.2 eviction trace **and** reword the `#[error]` string, which is true-for-all-three and non-breaking. Splitting the variant is breaking |
| 2.4 | Per-IP cap of 4 behind NAT | **LARGELY RESOLVED** (documented + configurable) | `intro_queue.rs:9-15` states the intent; `INTRO_MAX_PER_SOURCE` marked `configurable` at `SPEC.md:1270`; knob at `config.rs:162` | **y** (trivial) | Optional: one clause in §6.3's cap bullet stating the shared-public-IP consequence, where §6.4's NAT sentence (`SPEC.md:1293-1295`) covers only dedup |
| 3.1 | Datagram p99 22 ms on loopback at 0 % CPU | **INVESTIGATE-LATER**; premise partly unsupported | `D-real-socket.md:149,180`; **the 0 % CPU samples are from the idle window only** (`sample_cpu.sh`, `D:165`, `cpu-samples.log`) — none during phase C. `send_to` is sequential (`driver.rs:891-902`) but the loop is length 1 in ping-pong and 22 ms is ~10⁴× a loopback syscall | n | Split phase C's timer into `t1−t0` (send) and `t2−t1` (wake); 1000 iters, sub-second. Harness is uncommitted scratch — recreate first |

### Reading of the table

**Nothing here blocks a release and nothing moves a wire byte.** Every
"needs a ruling" is about a *document*, an *error string*, or a *status*
— none is a protocol change.

**Two items resolved for free** since `6448e6f`: the spin detector (2.1,
by rulings 249(ii)+255) and the NAT cap concern (2.4, which was already
deliberate and tunable). **Three new findings** the audit did not have:
2.1b, 2.2b, and the phase-C CPU-sampling correction in 3.1.

**Ranked for work.** 1.5 first — it is the only item where a named
protocol behaviour is provably unobserved by 1069 tests, and the fix is
one test. Then 1.7 (one token, restores a security assertion). Then 1.6
and 2.2b (one small test each). Then 1.1+1.4 as a single larger test.
Then the three rulings: 2.3 (real operator impact), 1.3 (the spec's own
text is in an unresolved state), 2.4 (cosmetic). 1.2 and 3.1 last.

**One pattern worth naming.** Four of the seven coverage items — 1.5,
1.7, 2.2b, and 1.6's near-miss — are the *same* defect: **a test named
for a property that does not separate the property**, or a guard whose
only reachable failure mode is the one nobody asserted. Working rule 9
predicts exactly this class, and the productive question in each case was
not "is there a test?" but "what would the broken build do here?" For 1.5
that question was cheaper to answer by *running* the broken build than by
reading the fixture — and it is what turned a suspicion into a fact.

# Round 40 rule-4 sweep — rulings 249–254

> **[Integrator's note, 2026/08/17, added at landing.]** This sweep was
> produced before the ruling numbers were assigned and its section
> headers guessed a different mapping. Match by topic, not by the numbers
> in the headers below: **B1 → ruling 254** (the cap), **B2 → 249** (the
> Pto disarm), **B3 → 250** (pump/coalescing), **B4 → 253** (reassembly),
> **B5 → 252** (accept), **B6 → 251** (rekey). The hit numbers
> **[S-nn]**, which rulings.md's round-40 entries cite, are stable and
> unambiguous. Two counts in the body were refuted at landing and are
> corrected here rather than edited in place: hit 89's "74 `accept()`
> sites in `tests/`" is wrong — the quoted command returns **31 across
> five files** at `6448e6f` (the qualitative finding, single-accept
> everywhere, stands); and B7's "eight `src/` sites" / "10 sites, 5
> files" undercount the actual **15 `212(c)` lines across four `src/`
> files** — grep, do not count from the table. Nothing else in this file
> was altered.

Base commit: `6448e6f` ("Round 39: persist the review reports and the resume point"), tree clean.
Verified with `git log --oneline -1` before any read.

Legend per hit: **CONTRADICTS** (prose argues the old position) · **DEPENDS**
(states a consequence of the old value/behaviour that becomes false) ·
**BENIGN** (mere mention, still true after the change).

---

## B1. PTO_BACKOFF_CAP 2^6 -> 2^3 (ruling 250)

### SPEC.md

1. **`SPEC.md:4516`** — CONTRADICTS (value)
   > anchored at the last ack-eliciting send, doubled per consecutive
   > unanswered probe (`2^pto_count`), capped at `PTO_BACKOFF_CAP` = 2⁶.

   The primary normative statement. Must move to 2³.

2. **`SPEC.md:4518–4521`** — CONTRADICTS (rationale)
   > The
   > probe train is ended by liveness (`DEAD_TIMEOUT` — under symmetric loss
   > and, since the anchor is the receive clock, under asymmetric loss too,
   > §7.4) — the cap is an overflow
   > guard, not a death sentence.

   This is *the* rule-4 hit: the sentence argues that the cap needs no
   operational tuning because liveness ends the train, i.e. it is the prose
   that justifies picking a value as large as 2⁶. Ruling 250 must address
   it directly, not merely rewrite the digit two lines above. Note the
   phrase "overflow guard" is exactly the search term the brief names, and
   this is its only occurrence in `SPEC.md`.

3. **`SPEC.md:4540`** (§13.3's own constant table) — CONTRADICTS (value)
   > | `PTO_BACKOFF_CAP` | 2⁶ |

4. **`SPEC.md:4702–4703`** (§14.4 persistent congestion) — DEPENDS
   > with
   > the backoff included, the threshold would run up to 2⁶× too long and
   > persistent congestion would never trigger under exactly the sustained
   > loss it exists to detect.

   The magnitude "2⁶×" is a stated consequence of the cap and becomes false
   at 2³×. **The conclusion survives** (8× is still "too long"), but the
   number does not, and the force of the argument for excluding the backoff
   from `persistent_period` weakens by a factor of 8 — worth a sentence in
   the ruling, because this is the only place in the spec that *derives* an
   arithmetic magnitude from the cap.

5. **`SPEC.md:7540`** (Named-constants table) — CONTRADICTS (value)
   > | `K_INITIAL_RTT` / `PTO_BACKOFF_CAP` | 333 ms / 2⁶ | §13.1 / §13.3 |

6. **`SPEC.md:7576`** (Constants-the-spec-never-names table) — CONTRADICTS (value)
   > | `PTO_BACKOFF_CAP` | **64** | §13.3 | written "2⁶" |

7. **`SPEC.md:7586–7591`** (ruling 63's shape #1) — DEPENDS
   > 1. **A ratio or rate stated in prose** — `½ window`, `every 2nd`, `9⁄8`,
   >    `≤ 1 per s`, `2⁶` — needs an identifier **and a stated unit**.
   >    `PTO_BACKOFF_CAP` proves the point: "2⁶" is a multiplier (64) in
   >    §13.3's sentence and reads as an exponent (6) in the table, and an
   >    implementer who guessed wrong would back off 64× too little with
   >    nothing red to show for it.

   Every literal here (`2⁶`, `64`, "64× too little") is cap-valued. At 2³
   the worked example becomes 8 / "8× too little". This is a **pedagogical**
   passage whose whole point is the multiplier-vs-exponent hazard, and at
   2³ the hazard sharpens rather than disappears: `1u32 << 8` is still
   well-defined (256), so the *undefined-behaviour* symptom that made the
   error loud at 64 vanishes at 8 — a silent-wrong-value regime. Ruling 250
   should say so; see also hit 11 below, which is the compile-time guard
   that currently catches it.

### rulings.md — which rulings argue FOR 2⁶

The record contains **no ruling that chose the value 2⁶**. It was inherited
from RFC 9002 practice and only ever *interpreted*. The two passages that
touch it argue about the **reading**, not the magnitude:

8. **`.spec-v2-clean-slate/rulings.md:1025–1029`** — "Settled by reading, no
   ruling needed" (slice 0 block) — DEPENDS
   > **Settled by reading, no ruling needed.** `PTO_BACKOFF_CAP` = 2⁶ is the
   > **multiplier (64), not the exponent**: §13.3 caps `2^pto_count`, and
   > §13.5 says "2⁶× too long". The planner flagged it as ambiguous; the
   > sentence structure settles it. A compile-time
   > `assert!(PTO_BACKOFF_CAP == 1 << 6)` keeps both readings visible anyway.

   Two things. (a) The value and the named compile-time assertion both
   become false at 2³. (b) **The citation is wrong and always has been**:
   §13.5 is "Frames, never packets" (`SPEC.md:4572`) and contains no such
   phrase; the *"2⁶× too long"* sentence lives in **§14.4**
   (`SPEC.md:4702`). See CONFLICTS §C1 — this is a rule-11 "a citation is a
   claim about the cited text" defect sitting in the very passage that
   settled the constant's reading.

9. **`.spec-v2-clean-slate/rulings.md:1046–1057`** — ruling 63 — DEPENDS
   > `PTO_BACKOFF_CAP` is the proof: "2⁶" is a multiplier (64) in §13.3's
   > sentence and reads as an exponent (6) in the table. An implementer who
   > guessed wrong backs off 64× too little **with nothing red to show for
   > it** — no test fails, because the constant is self-consistent.

   Same content as hit 7, in the ruling record. Rule 4(b) applies: ruling
   250 reverses the value ruling 63 uses as its worked example, so it
   inherits the duty to address 63's **reasoning**, not just the spec table
   63 produced.

10. **`.spec-v2-clean-slate/rulings.md:6420`** (ruling 223) — BENIGN
    > S12 observes §13.3's backoff by sampling `Network::sends() - tap().len()`

    About the observation technique, not the cap.

### src/

11. **`src/constants.rs:383–384`** — CONTRADICTS (value)
    > /// The cap on the PTO backoff **multiplier**, `2⁶`. §13.3.
    > pub const PTO_BACKOFF_CAP: u32 = 64;

12. **`src/constants.rs:607`** — DEPENDS (turns red)
    > const _: () = assert!(PTO_BACKOFF_CAP == 1 << 6); // the spec writes 2⁶

13. **`src/constants.rs:621–624`** — DEPENDS
    > // fidelity review. Ruling 63 named the hazard: a value written "2⁶" or
    > // "65 536 (2¹⁶)" is a judgement call an implementer can resolve wrongly
    > // and *self-consistently*, so nothing turns red. `PTO_BACKOFF_CAP` was
    > // already guarded; `REKEY_EPOCH_MSGS` is the identical shape and was not.

14. **`src/core/connection/recovery.rs:175–184`** — DEPENDS (one line turns red)
    > /// **Derived from the constant, never transcribed.** `PTO_BACKOFF_CAP` is
    > /// **64 — the multiplier, not the exponent** (`constants.rs`, and SPEC's
    > /// table writes it "2⁶"). v0.1 wrote `1u32 << pto_count.min(PTO_BACKOFF_CAP)`
    > /// with its own `PTO_BACKOFF_CAP = 6`; copying that idiom with slither's
    > /// constant shifts by up to 64, which is undefined behaviour on `u32`.
    > const PTO_MAX_EXPONENT: u32 = constants::PTO_BACKOFF_CAP.trailing_zeros();
    > const _: () = assert!(constants::PTO_BACKOFF_CAP.is_power_of_two());
    > const _: () = assert!(PTO_MAX_EXPONENT == 6);

    `PTO_MAX_EXPONENT == 6` is a hard compile-time pin and **will not
    compile** at 2³. The derivation itself (`trailing_zeros()`) is correct
    and needs no change — only the literal `6` and the doc prose. Also note
    the UB argument: at cap 8 the mis-transcribed idiom `1u32 <<
    pto_count.min(8)` is *legal* and merely wrong, so the failure mode this
    comment describes degrades from "UB / panic" to "silently wrong".

15. **`src/core/connection/recovery.rs:389`** — BENIGN (derived, stays true)
    > debug_assert!(multiplier <= constants::PTO_BACKOFF_CAP);

16. **`src/core/connection/tests_recovery.rs:1108–1145`** — CONTRADICTS
    (test name and body encode 64)
    > /// §13.3's `PTO_BACKOFF_CAP` = 2⁶, **stored as the multiplier 64**.
    > …
    > /// Six firings reach the cap, and the seventh and the
    > /// sixty-fourth must not move it — the sixty-fourth is the one that
    > /// detonates the bad idiom, so it is fired explicitly rather than
    > /// assumed.
    > fn the_pto_backoff_multiplier_caps_at_sixty_four() {

    The **test name** encodes the old value (rule 4: a name is part of the
    prose). Body asserts `INITIAL_PTO * 64` twice and loops `0..6`.
    Preceding test at :1099–1105 asserts 2×/4×/8× and stays true at 2³ *by
    coincidence* — at the new cap `8×` is the cap itself rather than a
    mid-train value, so that test silently stops separating "backs off" from
    "pinned at the cap". Rule 9 applies: it becomes a bound the degenerate
    (always-capped) implementation satisfies for free.

17. **`tests/spec_constants.rs:578`, `:584–585`** — CONTRADICTS (pin)
    > // SPEC.md §13.1 / Named constants: K_INITIAL_RTT / PTO_BACKOFF_CAP = 333 ms / 2^6.
    > assert_eq!(PTO_BACKOFF_CAP, 64);

    Note `:578` carries the cap in a comment on the **`k_initial_rtt`**
    test, where it is not asserted — a stale-comment site a grep for
    `assert_eq!(PTO_BACKOFF_CAP` would miss.

### Arithmetic elsewhere that derives from the cap

18. **`tests/story_reliability.rs:606–765`** (`s12_the_probe_train_backs_off…`)
    — BENIGN, but **rule 9 warning**. The window observes 4 probes at
    PTO ≈ 85 ms, so intervals are 85/170/340/(680). At cap 2⁶ none of the
    sampled intervals is capped; at 2³ the fourth is exactly the cap. The
    two assertions (`later > first`, `later*2 >= first*3`) use
    `probe_times[0..2]` and still pass. No change needed, but the margin to
    "the sampled window is entirely inside the cap" shrinks from 6 doublings
    to 3.

19. **`tests/story_reliability.rs:75–86`, `tests/story_message.rs:90–95`,
    `tests/story_datagram.rs:662–664`** — BENIGN
    > A PTO with no RTT sample yet is `K_INITIAL_RTT + 4·rttvar +
    > MAX_ACK_DELAY` = 333 + 666 + 25 ≈ 1 024 ms, and §13.3 doubles it per
    > unanswered probe, so three probes cost ≈ 7 s of virtual time.

    Three doublings from ≈1 024 ms is inside 2³, so `RECOVERY_PATIENCE`
    (12 s) and the 8 s datagram window are unaffected. Worth stating in the
    ruling that these were checked: they are the only virtual-time budgets
    in the suite sized off the doubling.

20. **`STORIES.md:71`** — BENIGN
    > *not* exponential backoff**. (§13's exponential PTO governs the

    Contrast against §5.5's handshake retransmit; cap-independent.

### Not found

No §7.4/§7.5 arithmetic derives a survival time or probe count from the
cap; the only cap-derived magnitude anywhere in `SPEC.md` is §14.4's "2⁶×"
(hit 4). `grep -n "64"` across §13 finds `MAX_ACK_RANGES` and
`DATAGRAM_*_QUEUE` only — unrelated.


## B2. Pto disarmed under the §7.3 amplification budget (ruling 249)

### SPEC.md — statements that Pto arming turns on the sent map alone

21. **`SPEC.md:4528–4530`** (§13.3) — DEPENDS, and a **rule-8** hit
    > **The `Pto` timer is armed only while at least one ack-eliciting packet
    > is in the sent map** (RFC 9002 §6.2.1); when the map empties it is
    > disarmed, and when the `Loss` timer is armed it takes precedence (§16.5).

    Bolded as *the* arming precondition. "Armed only while X" is a necessary
    condition and survives a second one being added, but the following
    clause ("when the map empties it is disarmed") reads as the **complete**
    disarm rule, and the implementation took it as an iff — see hit 26.
    This is exactly rule 8's shape: a stated construction whose scope is
    unstated, read as exhaustive.

22. **`SPEC.md:4560–4563`** (§13.4, ruling 221's paragraph) — DEPENDS
    > The probe
    > remains subject to §7.3's budget (below), which is what bounds the
    > re-offer: a budget with no room for a 39-byte challenge datagram emits
    > nothing, and the session dies at `DEAD_TIMEOUT` as §7.3 intends.

    States the **old** mechanism explicitly: the timer fires, the pump
    produces nothing, the connection dies at `DEAD_TIMEOUT`. Under 249 the
    timer does not fire at all in that state. The *outcome* survives (death
    at `DEAD_TIMEOUT`) but the described mechanism does not, and this
    sentence is currently the spec's clearest statement of "fires and emits
    nothing". Ruling 249 must rewrite it, not just §13.3.

23. **`SPEC.md:4568–4570`** (§13.4) — BENIGN, and the load-bearing premise
    > Probes are **not** exempt from §7.3's
    > anti-amplification budget on an unvalidated address.

    Still true and is precisely what makes 249 necessary. Cite it, do not
    change it.

### SPEC.md — the roam seam: the strongest contradiction

24. **`SPEC.md:4633`** (§13.6, ruling 173's exhaustive roam table) —
    **CONTRADICTS**
    > | PTO / loss detection | **undisturbed** — timers continue, no re-arm, no cancel | §13.3, §13.4 |

    Four lines above it, the same table says:
    > `SPEC.md:4629` | amplification byte counters (sent, received) | **reset to zero**, and the address becomes unvalidated | §7.3 |

    A roam zeroes the budget, so immediately post-roam the budget admits
    **nothing** until an authenticated window-fresh receive. Ruling 249 says
    `Pto` is disarmed in exactly that state — i.e. cancelled at the roam and
    re-armed on the first qualifying receive. The row says "no re-arm, no
    cancel". This is the single most important hit in the sweep: the roam is
    not a corner case for 249, it is the **generator** of the state 249
    governs, and the table that would have to change is one ruling 173
    ratified as *exhaustive*.

25. **`SPEC.md:4587–4589`** (§13.6 prose) — CONTRADICTS (same claim, prose form)
    > **Roaming** (§7.3): the sent map is **kept** — ACKs for packets in flight
    > to the old address still resolve, and `bytes_in_flight` remains consistent
    > with the retained map; loss detection and PTO continue undisturbed.

    Rule 4(a): the *other clause of the same sentence* as the table row's
    subject. Fixing the table and leaving this is the documented failure
    mode.

25b. **`SPEC.md:4607–4609`** (§13.6 parenthetical) — DEPENDS
    > a bounded stall of at most one loss-detection/PTO cycle, kept
    > probeable by the PTO exemption within §7.3's budget.

    "Kept probeable by the PTO exemption within §7.3's budget" assumes the
    PTO keeps cycling post-roam. Under 249 the post-roam stall is bounded by
    the **receive** that refunds the budget, not by a PTO cycle.

### SPEC.md — §16.5 / §16.4 timer contract

26. **`SPEC.md:5975–5978`** (§16.5 named-timer table) — DEPENDS
    > `Pto` is armed only while an
    > ack-eliciting packet is in the sent map (§13.3) — the probe is
    > ack-eliciting and is in that map (§13.5), so `Pto` and `Contested` can
    > be armed together and are independent;

    Two consequences become false. (a) The arming rule is restated here as
    the sole one — §16.5 is the timer contract, so this is the site an
    implementer builds the timer table from. (b) The word **"independent"**:
    under 249, `Pto`'s arming and `Contested`'s arming are both keyed to the
    same §7.3 budget predicate, so they are no longer independent in the
    sense this clause asserts — they become two timers gated by one
    condition, which is exactly the kind of coupling §16.5's
    equal-deadline-priority list exists to disambiguate.

27. **`SPEC.md:5849–5862`** (§16.4, `Contested` emission) — BENIGN, and the
    **precedent ruling 249 should cite**
    > - **`Contested` is emitted at the probe's transmission**, not at the
    >   mark. … a probe §7.3's amplification budget will not yet admit leaves the
    >   mark *pending*, which is reachable exactly when the connection has
    >   just roamed to an unvalidated address, not an exotic corner for a
    >   mobility-first protocol — and an event fired at the mark would
    >   announce a countdown that is not running.

    Unchanged by 249 and directly analogous: the spec already refuses to arm
    a *deadline* for output the budget will not admit. Ruling 249 is that
    principle applied to `Pto`. Naming it costs nothing and pre-empts the
    "this is new machinery" objection.

28. **`SPEC.md:5985–5987`** (§16.5 idempotence) — BENIGN, but check
    > **`handle_timeout` is idempotent**: each due timer is stopped before its
    > logic runs, so spurious or repeated calls no-op. For `Loss`/`Pto` the
    > idempotency additionally rests on synchronous sealing (§16.7).

    Still true. Worth stating in 249 that a **suppressed-then-re-armed**
    `Pto` does not break it: the re-arm is triggered by a receive, never by
    the firing itself, so the "firing re-arms itself in the past" class the
    driver guards (hit 39) stays unreachable. **Measured, not argued**:
    `src/shell/driver.rs:273` clears `last_timeout` on every non-`Timeout`
    event, and the budget can only grow on a receive, so the re-arm always
    lands on a pass where `last_timeout` is `None`.

### rulings.md — ruling 139's reasoning

29. **`.spec-v2-clean-slate/rulings.md:3500–3502`** (ruling 139(a)) —
    **CONTRADICTS its own rationale**
    > (a) `pto_count` increments **at the `Pto` timer's firing**, before the
    > probe is built — RFC 9002's point, and the one that stays right in slice
    > 7 where §7.3's budget can prevent a probe leaving.

    The **entire slither-specific justification** for increment-at-firing is
    the budget-blocked case, and 249 removes that case from the firing path
    (a suppressed firing never happens). Ruling 249 says 139(a) "keeps its
    role only for non-suppressed firings" — which is the *conclusion*
    surviving while its *stated reason* evaporates. Rule 4(b) is explicit
    that a ruling reversing another inherits the duty to address its
    **reasoning**; here the conclusion is retained and the reasoning must be
    re-founded on RFC 9002's own point alone. Ruling 175/188's defect
    (correcting a number while silently reinstating a characterisation) is
    the shape to avoid.

    Note also the **behavioural** consequence 249 should state out loud:
    under the old design the backoff kept doubling while the budget was
    empty, so the first admitted probe after a long block went out at a
    heavily backed-off interval; under 249 the count is frozen across the
    suppressed window. Combined with B1's cap drop to 2³ this changes the
    post-roam probe cadence twice over, in the same round.

30. **`.spec-v2-clean-slate/rulings.md:4432`, `:4442–4443`, `:4675`** —
    BENIGN (all cite 139(b), the persistent-congestion clause).

### src/

31. **`src/core/connection/recovery.rs:359–366`** — DEPENDS (doc states old rationale)
    > /// **[ruling 139(a)]** The increment is at the *firing*, before the
    > /// probe is built — RFC 9002's point, and the one that stays right in
    > /// slice 7, where §7.3's anti-amplification budget can stop a probe
    > /// leaving and an increment tied to transmission would stall the
    > /// backoff at an unvalidated address.

    A verbatim carry of ruling 139(a)'s rationale into the code. "An
    increment tied to transmission would stall the backoff at an unvalidated
    address" — 249 *deliberately* stalls it there (by not firing), so this
    comment argues against the new rule.

32. **`src/core/connection/recovery.rs:376–382`** — **CONTRADICTS** (iff)
    > /// `None` iff the sent map is empty — §13.3's precondition, *"the `Pto`
    > /// timer is armed only while at least one ack-eliciting packet is in the
    > /// sent map"*.

    The **iff** is the code's reading of hit 21, and it is the sentence 249
    falsifies. `pto_deadline()` (`:383–396`) returns `Some` whenever the map
    is non-empty and an anchor exists; it has no access to the budget at
    all, so 249 needs either a budget argument threaded here or the disarm
    applied at the `timers.set` call site.

33. **`src/core/connection/mod.rs:1496–1498`** — DEPENDS (the arming site)
    > .set(TimerKind::Loss, self.recovery.loss_deadline());
    > .set(TimerKind::Pto, self.recovery.pto_deadline());

    The only place `Pto` is armed. Whatever shape 249 takes, this is the
    line that changes.

34. **`src/core/connection/mod.rs:689–694`** — DEPENDS
    > // §13.3's firing. `pto_count` increments **here**, before
    > // the probe is built (ruling 139(a)).
    > TimerKind::Pto => {
    >     self.recovery.on_pto_timeout();
    >     probe = true;
    > }

35. **`src/core/connection/tests_recovery.rs:1083–1084`** — BENIGN (name),
    DEPENDS (comment)
    > /// §13.3: *"doubled per consecutive unanswered probe (`2^pto_count`)"*,
    > /// with ruling 139's increment moment — **at the timer's firing**.
    > fn each_pto_firing_doubles_the_interval()

    Unit-level, drives `Recovery` directly with no budget in scope; stays
    green. Listed because it is the only test whose *name* pins 139(a)'s
    moment, and the sweep brief asks for them.

36. **`src/core/connection/timers.rs:40`, `:152`, `:180`, `:191`, `:204`** —
    BENIGN. `Pto`'s ordering against `Loss` and the post-mortem disarm are
    untouched by 249.

### Not found

No test in `src/` or `tests/` names ruling 249's state (a Pto suppressed by
the budget). Working rule 13 applies: `FlakyWire` can construct it — a roam
to a fresh source zeroes the budget — but nothing currently asserts on the
timer's arming there, only on the eventual death. The acceptance evidence
for 249 does not exist yet.


## B3. §7.3 arithmetic restated at pump time; probe may coalesce PATH_CHALLENGE/RESPONSE (ruling 251)

### SPEC.md — the two "always" claims 251 restates

37. **`SPEC.md:2418–2423`** (§7.3, ruling 171/208) — **CONTRADICTS** ("never
    contend … under any budget this protocol can construct")
    > Between themselves the order is
    > free: each costs 9 bytes of frame, so a packet carrying **both** costs
    > 14 B of header + 18 B of frames + a 16 B tag = 48 B, and the smallest
    > budget any arming can produce is 3 × the 30-byte keepalive that armed it =
    > 90 B (§7.5, ruling 203's arithmetic). They therefore never contend with
    > one another under any budget this protocol can construct

    The 90 B figure is the budget **at the arming instant**. By pump time the
    budget has been spent down by whatever left since — which is the whole
    point of restating the arithmetic at pump time. "Never contend under any
    budget this protocol can construct" is a universal over *armed* budgets,
    not over *remaining* budgets, and the sentence does not say so (rule 8:
    a stated construction with an unstated scope).

38. **`SPEC.md:2448–2451`** (§7.3, ruling 215's flag text) — CONTRADICTS
    > it yields when the budget cannot hold both, **and here the budget can**: probe
    > and challenge together cost 14 B of header + 1 B of PING + 9 B of
    > challenge + a 16 B tag = **40 B**, inside the 90 B floor computed above.

39. **`SPEC.md:2462–2464`** (§7.3, ruling 215's operative sentence) —
    **CONTRADICTS**, and the sharpest one
    > and **the send pump may
    > emit the probe and continue building on the same pass**, because the
    > budget holds both and always does: 40 B against a 90 B floor.

    *"and always does"* is the exact universal 251 replaces with a pump-time
    check. This is the sentence to rewrite; the two above are its premises
    and must move with it (rule 4(a) — the other clauses of one argument).

40. **`SPEC.md:2273`** (§7.3) — DEPENDS (the 90 B floor's single origin)
    > can arm the budget — §7.5's 30-byte keepalive, 90 B — still admits a

### SPEC.md — two sites that still call ruling 215's flag OPEN

41. **`SPEC.md:4758–4760`** (§14.5) — **CONTRADICTS**, high severity
    > `PATH_RESPONSE`
    > and `PATH_CHALLENGE` rank immediately below the probe and above the pure
    > ACK, and §7.3 carries a flagged, unresolved interaction between the probe's
    > rank and the challenge's — read the order there, not here.

    §7.3 carries **no** unresolved interaction: `SPEC.md:2440–2441` stamps it
    `[RATIFIED 2026/08/16 — ruling 215, closing this section's one open flag.
    §1.3's expected flag count returns to zero.]`, and §1.3 (`SPEC.md:493`)
    says the marker *"now appears nowhere in this document"*. §14.5 still
    sends the reader to read an open flag that does not exist.

42. **`SPEC.md:4953`** (§15.4 teardown matrix, the **contested** row) —
    **CONTRADICTS**, same defect
    > §7.3 also carries a flagged, unresolved interaction between this rank and ruling 208's `PATH_CHALLENGE`

    41 and 42 together are **ruling 215's own named process defect happening
    to ruling 215**: *"closing a flag is not sweeping the spec … the
    authoritative artefact was left behind by an update to the record about
    it."* Ruling 215 explicitly said *"§14.5 needs no change, because the
    ranks it describes were right all along"* — the **ranks** were, the
    **flag reference in the same sentence** was not (rule 4(a)). Ruling 251
    is the natural place to sweep both.

### rulings.md — 212 / 215 / 217

43. **`.spec-v2-clean-slate/rulings.md:6204`** (ruling 215's title) — DEPENDS
    > ### 215 — ruling 212(c) is reversed in its rank half. The ranks stand; the pump was always the defect.

44. **`.spec-v2-clean-slate/rulings.md:6232–6237`** — **the sentence 251 must
    address** (rule 4(b))
    > **The flag offered three resolutions and I took the weakest.** They were:
    > coalesce the challenge into the probe's packet; lift the challenge above
    > the probe; or **hold that the priority order was never an early return and
    > the pump is simply wrong.** The flag all but names the third as correct

    Ruling 251 permits **resolution 1** (coalescing) — the one 215 listed and
    passed over. 215 gave no argument against it, so 251 does not contradict
    215's reasoning, but it adopts an option 215 characterised as not the
    correct one. The honest statement for 251: *215 declined resolution 1
    without argument; 251 adopts it because a pump-time budget makes two
    packets a contention 215 assumed away.*

45. **`.spec-v2-clean-slate/rulings.md:6243–6246`** — DEPENDS
    > It may emit the probe *and* continue building, because the budget holds both
    > and always does. This concedes nothing: the probe keeps the liveness
    > priority §7.5 proves it needs, and the challenge is still built on the
    > same pass.

46. **`.spec-v2-clean-slate/rulings.md:6247–6252`** — BENIGN (212(c)'s
    surviving half: the early return is wrong as written).

47. **`.spec-v2-clean-slate/rulings.md:6320`** — BENIGN
    > **Ruling 217's `dedicated_sent` machinery is deleted, not wired up.**

    Ruling 217 leaves no live packet-per-frame claim; nothing to sweep.

### src/ — the pump still implements 212(c), which ruling 215 reversed

48. **`src/core/connection/mod.rs:2055–2069`** — **CONTRADICTS `SPEC.md` as
    it stands today, before ruling 251 is applied.** Highest-severity finding
    of the sweep.
    > // **[ruling 212(c)]** §7.3's ranks 2 and 3 — `PATH_RESPONSE` and
    > // `PATH_CHALLENGE`, immediately after CLOSE and **above** the
    > // contested probe. *"Everything else in the order competes for the
    > // budget; the challenge dissolves it. …"*
    > //
    > // **This pre-pass exists because the probe's early return below
    > // would otherwise deny them**, which ruling 212(c) names as wrong
    > // as written

    Current `SPEC.md` §7.3 (`:2397–2408`) ranks them **1 CLOSE, 2 contested
    probe, 3 `PATH_RESPONSE`, 4 `PATH_CHALLENGE`**, and ruling 215 says those
    ranks *"stand exactly as written"*. The code's comment calls the path
    frames "ranks 2 and 3" and puts them **above** the probe, and the
    behaviour follows the comment: `pump_path_frames` emits a whole separate
    datagram before `pump_contested_probe` runs. On a pending mark at an
    unvalidated address the implementation therefore transmits
    **challenge-then-probe** — 212(c)'s order, not §7.3's.

49. **`src/core/connection/mod.rs:2071–2085`** — DEPENDS
    > // **[ruling 171]** Rank 4. A pending probe the budget cannot admit
    > // stops everything **below** it
    > if !self.pump_contested_probe(now) { return; }

    "Rank 4" is 212(c)'s numbering for the probe; ratified §7.3 makes it rank
    2. The early return survives only in its *refused* form (the pump
    continues when the probe is admitted), which is 215-conformant — but the
    pre-pass above it is not, and the rank label is wrong.

50. **`src/core/connection/mod.rs:2565–2578`** — DEPENDS, plus an
    **unanswered question 251 must answer**
    > /// One packet carrying nothing but §7.3's path frames, for the one state
    > /// in which the pump's loop cannot carry them: a **pending** contested
    > /// probe (**[ruling 212(c)]**).
    > …
    > /// Sealed `seal_quiet` and **counted** in the sent map: the frames are
    > /// ack-eliciting (§8.3) and §14.5's exemption list — PTO probes, the
    > /// contested probe, non-ack-eliciting control packets — does not name
    > /// them, so unlike the probe beside it this packet is gated by the
    > /// congestion window as well as by the budget.

    This dedicated datagram is exactly what coalescing removes. But the two
    packets face **different congestion gates**: the probe is cwnd-exempt
    (§14.5), the path-frame packet is not. Coalescing merges a cwnd-exempt
    packet with a cwnd-gated one, and **nothing in the current spec says
    which gate the merged packet faces.** Ruling 251 must state it or it
    ships a rule-8 gap.

51. **`src/core/connection/mod.rs:2546–2551`** (`pack_path_frames`) — DEPENDS
    > // Answering an obligation before raising one — §7.3: *"between
    > // themselves the order is free"*, and this is the conventional
    > // reading. They never contend: 14 B of header + 18 B of frames + a
    > // 16 B tag = 48 B, inside the 90 B floor the smallest arming funds.

    The code carries hit 37's universal verbatim.

52. **`src/core/connection/mod.rs:2010–2046`** (`packing`) — BENIGN, and the
    mechanism 251 leans on (rule 11: opened and verified)
    > **[ruling 207(c)]** [`Amplification::room`] is in **datagram** bytes;
    > [`Packing`]'s budget is in **plaintext** bytes. They differ by exactly
    > §3.4's `DATA_HEADER_LEN + AEAD_TAG_LEN` = 30

    `packing()` already sizes each packet to the **remaining** room, so the
    pump-time restatement 251 wants has a mechanism that exists.

### The three tests that encode challenge-first

53. **`src/core/connection/tests_path.rs:1062`
    `the_challenge_outranks_the_contested_probe`** — BENIGN in behaviour,
    **CONTRADICTS in name**.
    The body asserts only intra-packet order (`challenge_at < ping_at`), and
    the doc at `:1049–1060` already re-founds it on §8.5 after ruling 215:
    > **[Integrator, ruling 215 — resolved, and this test does NOT invert.]**
    > … Under it
    > the probe outranks the challenge for admission **and** the challenge
    > still precedes the PING in the bytes

    Under 251's coalescing both frames land in **one** packet, so the
    assertion becomes more meaningful, not less. The **name** still asserts
    the rank ruling 215 reversed — rule 4: a name is prose, and this is what
    the next blind author reads as a design disagreement.

54. **`src/core/connection/tests_contested.rs:684–692`** (inside
    `the_probe_the_notification_and_the_deadline_all_land_at_the_transmission_instant`)
    — **DEPENDS; this test goes red under 251.**
    > // **[I1, ruling 212(c)]** Two packets, and their **order** is the
    > // ruling … The address is
    > // unvalidated, so the released room buys the challenge first (39 B) and
    > // the probe second (31 B) — inside the 90 B a single keepalive credits.
    > assert_eq!(d2.transmits().len(), 2, "the challenge and the probe, in that order");

    Coalescing makes this **1**. The comment also states 212(c)'s reversed
    rank as settled fact.

55. **`src/core/connection/tests_contested.rs:1092–1098`** — DEPENDS
    (comment only; the assertion survives)
    > // **[I1, ruling 212(c)]** The probe is no longer the *first* packet
    > // sealed after the mark: §7.3 ranks `PATH_CHALLENGE` above it, so the
    > // challenge takes the floor counter and the probe takes the next one.

    Under coalescing there is one packet and one counter, so the probe takes
    the floor counter again; `highest >= floor` still passes and the comment
    becomes false. Exactly rule 4's shape: the assertion is safe, the prose
    is not.

56. **`src/core/connection/tests_path.rs:1005`
    `a_pending_contested_probe_does_not_block_the_challenge`** — BENIGN. Its
    doc says it asserts only *"what both readings agree on"*. Survives
    coalescing.

57. **`src/core/connection/tests_path.rs:951–985`** — DEPENDS (a 35-line
    reported-conflict header written to 212(c), still live)
    > // ⚠ **CONFLICT, REPORTED AND NOT RESOLVED (working rule 3).**
    > // … This author's brief names 212(c), so the tests
    > // below are written to 212(c) — and the conflict is reported rather than
    > // silently picked.

    Worth keeping as history, but it nowhere says the conflict was
    **resolved by ruling 215** — only the single test at `:1049` does.

58. **`src/core/connection/testfix.rs:793–795`** — BENIGN (citation only)
    > Ruling 212(c) is exactly such a statement, and it
    > is unassertable through `pump_from`.

59. **`src/core/connection/tests_contested.rs:876`
    `a_pending_probe_outranks_a_queued_datagram_when_the_budget_admits_only_one`**
    — BENIGN. Probe versus rank 9; untouched by 251.


## B4. Reassembly small-to-large merge; §10.6 ceiling ~credit; cost note (ruling 254)

### SPEC.md

60. **`SPEC.md:4218–4221`** (§10.6, the mandate) — BENIGN, and the sentence
    254's ceiling clause must remain true of
    > The mandate: **per-stream reassembly state
    > MUST be O(advertised credit) and MUST NOT scale with the number of
    > received frames.**

    Unchanged by a small-to-large merge — but note that "state" is *bytes
    plus metadata*, and an amortised-growth `Vec` can hold up to **2×** the
    span in capacity. If 254 tightens capacity accounting so it stays
    ≈ credit, this is the sentence it is protecting; if it does not, this
    mandate is where the slack shows up.

61. **`SPEC.md:4222–4227`** (§10.6, admissible option (b)) — DEPENDS
    > (b) the default — received ranges are coalesced on
    > insert, and a stream whose stored discontiguous ranges would exceed
    > `REASSEMBLY_CHUNKS_MAX` (= 1024) after coalescing is a protocol
    > violation: CLOSE with `PROTOCOL_VIOLATION` (§15.3; quinn's
    > defragment-plus-hard-fail shape).

    "Coalesced on insert" is preserved by small-to-large; the *cost* of that
    coalesce is what changes, and §10.6 currently says nothing about it —
    which is exactly the gap 254's cost note fills.

62. **`SPEC.md:4227–4229`** — BENIGN, still true
    > The ceiling value ships
    > ratified-but-revisitable, gated on the Appendix B
    > defragmentation/throughput check.

63. **`SPEC.md:4245–4251`** (§10.6, ruling 94's reconciliation) — DEPENDS
    > Both bounds are real and they are reconciled by never allocating ahead of
    > arrival. … Buffering only what has arrived makes total buffered bytes across **all**
    > streams bounded by the advertised connection credit, and (b)'s
    > coalesce-on-insert keeps each stream's range count bounded independently.

    *"Never allocating ahead of arrival"* is the invariant a growth-amortised
    merge bends: `Vec::reserve`/`extend` allocates headroom for bytes that
    have **not** arrived. The claim survives only if 254's tightened
    accounting keeps capacity at the arrived span (e.g. exact `reserve_exact`
    or a shrink on merge). This sentence is the one that makes it a
    correctness question rather than a performance one.

64. **`SPEC.md:4254–4256`** (§10.6) — DEPENDS, and the reason the ruling
    must think about `capacity()`
    > *A test for this must assert allocated **capacity**, not bytes received.*
    > An eager per-stream allocator receives few bytes and passes a
    > bytes-received assertion for free.

    §10.6 makes **capacity** the normative observable. Any merge strategy
    that changes `Vec::capacity()` behaviour changes what this sentence
    measures. See hits 70–71 for the two tests that read it exactly.

65. **`SPEC.md:3751–3754`** (§9.5's bullet) — BENIGN
    > Reassembly memory is bounded twice over: by advertised credit (the span
    > a receiver must cover, §10.6) and by the reassembly-fragment mandate of
    > §10.6 — per-stream reassembly state MUST be O(advertised credit) and
    > MUST NOT scale with the number of received frames.

    A second, cross-referencing statement of hit 60. Rule 4: if 254 amends
    §10.6's ceiling wording, this bullet is the site a token-grep for
    "REASSEMBLY_CHUNKS_MAX" would miss — it names neither the constant nor
    §10.6's option letters.

66. **`SPEC.md:7105–7107`** (Appendix B) — BENIGN, and the natural home for
    the cost note's acceptance evidence
    > **The reassembly-fragment bound** (§10.6): a one-byte-frames-at-
    > even-offsets flood stays O(credit) or dies at `REASSEMBLY_CHUNKS_MAX`
    > with `PROTOCOL_VIOLATION`; the defragmentation cost is measured by the
    > throughput gate below.

67. **`SPEC.md:7499–7504`** (Appendix B throughput gate) — BENIGN
    > measured with the §10.6 reassembly bound active (the
    > defragmentation/coalescing cost is part of the number)

    Already obliges the merge cost to be measured. `benches/throughput.rs`
    exists (round 39); if 254 adds a cost note it should say whether this
    gate is the evidence or whether a new one is owed.

### rulings.md

68. **`.spec-v2-clean-slate/rulings.md:2143–2152`** (ruling 94) — DEPENDS
    > **Ruling 94 — reassembly allocates lazily, coalesces on insert, and is
    > bounded connection-wide.** … Slice 4
    > implements option **(b)** — coalesce-on-insert, `REASSEMBLY_CHUNKS_MAX` =
    > 1024 — allocating only on arrival, so both bounds hold at once.

    *"Allocating only on arrival"* is 94's operative promise; small-to-large
    with amortised growth allocates slightly ahead of it. Rule 4(b): 254
    inherits the duty to say whether 94's promise is preserved exactly or
    relaxed to "within a constant factor".

69. **`.spec-v2-clean-slate/rulings.md:2154–2160`** (ruling 94) — DEPENDS
    > **The test must assert allocated capacity, not bytes received.** An
    > eager per-stream allocator receives few bytes and would pass a
    > bytes-received assertion for free — working rule 9's exact trap

    The **"revisitable"** sentence the brief asks for is **not** in ruling 94.
    It is at `.spec-v2-clean-slate/rulings.md:194–196` (rulings 19–22)
    > O(credit) reassembly bound (REASSEMBLY_CHUNKS_MAX 1024, revisitable).

    and re-argued at `:2380–2384` (ruling 103):
    > `REASSEMBLY_CHUNKS_MAX` (§10.6) is a *third* kind: receiver policy like
    > the batch, but **externally observable**, because a peer that fragments
    > past one receiver's ceiling is killed and past another's is not. It is
    > shipped "ratified-but-revisitable" and is a tolerance.

    BENIGN for 254 (the ceiling **value** does not move), but load-bearing:
    ruling 103 makes the ceiling *externally observable*, so any merge change
    that alters **when** the 1024 count is reached is a wire-visible change,
    not an implementation detail. Small-to-large does not alter the count —
    verified by reading `recv.rs:582–586`, where the check is on
    `self.chunks.len()` after the merge — but 254 should say so out loud.

69b. **`.spec-v2-clean-slate/rulings.md:6121–6127`** (ruling 213(c)) — the
    **"left alone"** sentence the brief asks for lives in the code, not the
    ruling; ruling 213(c) states the same fact from the contract's side —
    > **(c) `CONTRACT-7b.md` §5 overstates its own invariant.** It claims the
    > work becomes *"bounded by bytes that are new to the buffer"*. Bounded by
    > the **existence** of a new byte, yes; **not linear in them** — a frame
    > bridging two stored chunks still copies the whole span for one new byte.
    > Defect class 1 in a contract this time: a stated construction with a scope
    > broader than what it delivers.

    — **DEPENDS**. This is the exact
    cost ruling 254 removes. 213(c) accepted it as a known, bounded,
    progress-making case; 254 says it is now cheaper. The ruling should
    record that 213(c)'s characterisation is superseded rather than merely
    improved, because 213(c) is cited as an instance of *defect class 1* and
    a reader will otherwise take the class example as still live.

### src/core/connection/recv.rs

70. **`src/core/connection/recv.rs:469–489`** (`insert`'s doc, F3 block) —
    **DEPENDS**, the sentence the brief asks for
    > It is *not* the claim that
    > the work is linear in the new bytes — a frame that bridges two stored
    > chunks still copies the merged span for one new byte. That case makes
    > progress, is bounded by credit, and is **left alone**; F3 is exactly the
    > **zero**-progress case.

    Explicitly accepts the whole-span copy. Under 254 the bridging case is
    no longer "left alone", so this paragraph argues the position being
    changed. Note also `:470–471`:
    > **[F3]** The merge below allocates and copies the whole merged span,
    > and the span is the *stored* chunk's, not the arriving frame's.

    — a second statement of the same fact inside the same doc comment
    (rule 4(a): both clauses move together).

71. **`src/core/connection/recv.rs:549–579`** (the merge branch) — DEPENDS
    (the code 254 replaces)
    > let mut merged = vec![0u8; (stop - start) as usize];
    > … for c in self.chunks.range(lo..hi) { merged[…].copy_from_slice(&c.data); }

    Whole-span fresh allocation plus a copy of every merged chunk. Two
    properties the small-to-large rewrite must preserve and which are only
    stated here, not in the spec: *(i)* **stored bytes win on overlap** —
    > // Stored bytes win on overlap — §9.5 lets the receiver keep
    > // either, and keeping the first is the cheaper invariant to
    > // reason about.

    A small-to-large merge that copies the *small* side into the *large* one
    inverts which copy survives whenever the arriving frame is the larger
    side. §9.5 permits either, so it is not a conformance break — but this
    comment claims a fixed policy, and `tests_reassembly.rs:190–192`
    deliberately uses **the same byte value** so as not to lean on it.
    *(ii)* the disjoint branch's `data.to_vec()` is an **exact-capacity**
    allocation (`recv.rs:551` comment says so).

72. **`src/core/connection/recv.rs:440–446`** — BENIGN, must stay true
    > // **Ruling 94: allocate lazily.** No `with_capacity` here — this is
    > // the line that would turn 128 peer-opened streams into 32 MiB.

73. **`src/core/connection/recv.rs:448–450`** — BENIGN (the accounting hook)
    > /// Bytes of allocated capacity. Ruling 94's test-visible accounting.
    > fn capacity(&self) -> u64 { self.chunks.iter().map(|c| c.data.capacity() as u64).sum() }

    Note it sums **capacity**, not length. Any headroom a small-to-large
    merge leaves is therefore directly visible in the §10.6 observable — the
    accounting tightening the brief mentions is *required*, not optional, or
    hit 74 goes red.

### The capacity-shaped tests

74. **`src/core/connection/recv.rs:693–703`** — **DEPENDS; goes red under an
    amortised-growth merge.** (The brief's "recv.rs:696 exact-capacity
    assertion" — it is at `:702` in this tree, in a test starting `:694`.)
    > /// Coalescing is what keeps the count down: 2000 adjacent ranges are one
    > /// chunk, not 2000.
    > fn adjacent_ranges_coalesce_rather_than_accumulate() {
    >     for i in 0..2_000u64 { half.apply_stream(i, b"x", false)…; }
    >     assert_eq!(half.capacity(), 2_000);
    > }

    2 000 single-byte adjacent inserts. Today each merge does
    `vec![0u8; span]`, so capacity is **exactly** the span. A small-to-large
    merge built on `Vec::extend`/`reserve` reaches capacity 2 048 (or
    whatever the growth policy yields) and this `assert_eq!` fails. It is the
    one exact-capacity assertion in the crate that the change touches, and it
    is *also* the test that would catch an accounting regression — so it must
    be **re-derived**, not relaxed to `>=`. Rule 9: relaxing it to an upper
    bound the degenerate implementation satisfies for free is the trap.

75. **`src/core/connection/recv.rs:661–674`** — BENIGN
    > fn capacity_is_what_arrived_and_not_the_window() { … assert_eq!(half.capacity(), 100); … }

    Single disjoint insert; exact `to_vec()` path is unchanged.

76. **`src/core/connection/recv.rs:741–752`** — BENIGN
    > assert_eq!(half.capacity(), 200);
    > assert_eq!(half.capacity(), 0, "§12.7: the discard is the first moment");

77. **`src/core/connection/tests_reassembly.rs:38–61`** (module header, the
    F3 separator) — **DEPENDS**, and the most fragile thing in B4
    > | **broken** — re-coalesces | `vec![0u8; N - n]`: capacity **collapses to the span** |
    > | **fixed** — early return | capacity **unchanged** |

    The whole separating argument is *"the re-allocated span has the same
    capacity as the chunk it replaced"*, which is a property of
    `vec![0u8; span]` — i.e. of the exact code 254 replaces. Under
    small-to-large the "broken" build's signature is no longer a **collapse**
    to the span, so this table stops describing either build. The test bodies
    still pass (they assert *unchanged*, and F3's early return still fires),
    but the documented mechanism that justifies them is void. This is ruling
    213(b)'s hard-won separator — the one two blind agents derived
    independently (ruling 214) — and 254 should re-verify it rather than
    assume it.

78. **`src/core/connection/tests_reassembly.rs:140–158`
    (`assert_no_reallocation`), `:175`, `:225`** — BENIGN in outcome
    > "{what}: reassembly capacity fell from {before} to {after}. …"
    > "{what}: reassembly capacity grew from {before} to {after}. …"

    Two-sided (fell **and** grew), which is why they survive: covered frames
    take the early return under both merge strategies. Listed because the
    "grew" half is what would catch an over-eager `reserve` if 254's
    implementation ever reached this path.

79. **`src/core/connection/tests_reassembly.rs:245–256`** — DEPENDS (prose)
    > **This test does not separate the builds and is not claimed to.** With
    > nothing ever read there is no slack, so the re-allocated span has the same
    > capacity as the chunk it replaced and `reassembly_capacity()` is blind to
    > it.

    Same stale mechanism as hit 77, restated per-test.

80. **`tests/spec_constants.rs:497` (`reassembly_chunks_max`)** — BENIGN. The
    ceiling value does not move.

81. **`benches/throughput.rs:246`** — BENIGN
    > reassembly buffer, the mean fill approaches 64 KiB and the await count

    Round 39's benchmark; the natural place to evidence 254's cost note
    against Appendix B's throughput gate (hit 67).


## B5. accept() obligation; lost-msg2 peer re-appears as fresh Intro (ruling 253)

**Framing, verified against §5.5 and §6.4 rather than assumed.** The
responder **never retransmits msg2** — `SPEC.md:962–968` makes *"every
retransmit … a completely fresh initiation — new ephemeral, new random
index, new strictly-greater timestamp"*, and `SPEC.md:1418–1420` adds that
*"a msg2 answering a superseded initiation is ignored"*. So a lost msg2
leaves the responder holding a LIVE, never-confirmed session and the
initiator re-offering a **new `Intro`** every ~5 s for 90 s. Only a second
`accept()` closes it, and by §6.4's §16.1 guard that accept **replaces**
the unconfirmed session (basis `Some(t)`, strictly-greater timestamp),
firing `ConnectionLost::Replaced`. Nothing in the spec says an application
must do this.

### SPEC.md — the obligation exists, under-scoped

82. **`SPEC.md:1675–1676`** (§6.5) — DEPENDS, and the **rule-8 hit**
    > **Applications that dial SHOULD
    > also drain `accept()`.**

    The nearest thing the spec has to ruling 253's obligation, and its
    scope is *dialling applications*, justified by the NAT-rewritten
    simultaneous-open false negative in the preceding sentences. A
    **pure responder** that loses msg2 needs the same obligation for an
    entirely different reason and this sentence does not reach it. Classic
    rule-8 shape: a stated construction with a scope narrower than the
    hazard.

83. **`SPEC.md:1613–1616`** (§6.4's `Stale` list) — BENIGN, and the model
    253 generalises
    > The application SHOULD
    > re-accept when the peer's next initiation surfaces as a new `Intro`.

    Already an "accept more than once" obligation — but conditioned on
    having received `AcceptError::Stale`. In the lost-msg2 case the first
    accept **succeeded**, so no error ever tells the application to try
    again. That gap is exactly what 253 closes, and this sentence is the
    template to widen.

84. **`SPEC.md:1857–1865`** (§6.8) — BENIGN, and the closest existing
    narrative
    > Restart needs no machinery of its own (§5.4): a restarted *peer*
    > reconnects, its initiation parks as an ordinary `Intro` at our LIVE
    > static, the live (now-zombie) connection keeps running untouched, and the
    > `Replaced` teardown fires only at the replacing `accept()` (§6.4)

    Structurally identical to the lost-msg2 case (LIVE connection, peer
    re-offers, replacement at the next accept) — and §6.8 says *"restart
    needs no machinery of its own"* **without** saying it needs an
    application that keeps accepting. Ruling 253's obligation is what makes
    §6.8's claim true; today it is load-bearing and unstated.

85. **`SPEC.md:1866–1872`** (§6.8) — BENIGN (basis analysis, still correct)
    > Where the zombie is a connection we **accepted**, its basis is `Some(t)`
    > and the restart replaces at the first `accept()`

    Confirms the lost-msg2 responder's session **is** replaceable: basis
    `Some(t)` because we accepted it. Rule 11: the mechanism 253 names
    exists — verified in §6.4 (`SPEC.md:1444–1450`) and §17.4.

### What is NOT there

86. **No statement anywhere that one `accept()` suffices**, and equally
    **no statement that it does not**. §6.2's verb table
    (`SPEC.md:1245`) and §6.1's ladder describe a single chain; §16.2's
    shell surface exposes `accept()` as a single future. The absence is
    the finding: an implementer reading §6 top to bottom builds a
    one-accept server and it works on every test in this repo (see 89).

### src/ and docs

87. **`src/lib.rs:9–11`** — BENIGN, but the crate's one-line model
    > Connections roam across address changes, rekey by
    > ratchet, and are accepted in *stages*, so an application can inspect a
    > peer's claimed identity before spending a second DH on it.

    "Accepted in stages" describes the DH ladder, not repetition. If 253
    documents an application obligation, the crate root is where a reader
    forms the mental model, and it currently forms an establish-once one.

88. **`src/shell/endpoint.rs:75–115`** (`Endpoint::accept` and
    `poll_accept`) — DEPENDS (documentation gap)
    > pub async fn accept(&self) -> Option<Intro<I>> { … }
    > /// `None` means the endpoint is closed, exactly as [`accept`](Self::accept)

    The rustdoc explains cancel-safety and ruling 229's poll seam and says
    **nothing** about the caller's obligation to keep calling. Ruling 253's
    documentation lands here. Note the doc example at
    **`src/shell/endpoint.rs:386`** is
    > /// // … dial and accept through `endpoint` …

    — a comment, so it cannot mis-teach a loop, but it also cannot teach
    one.

89. **`src/testutil/mod.rs:1217–1243`** (`Pair::establish`) — **DEPENDS,
    and this is working rule 13 in its exact form**
    > /// Dial `a` → `b` and drive §6.2's staged accept to completion,
    > /// returning both connections.
    > /// … `accept()` → `read_identity()` → `authenticate()` → `accept()`, run
    > /// concurrently with the dial so the paused clock advances.
    > let accept = async {
    >     let intro = self.b.endpoint.accept().await.expect("an introduction");
    >     …
    > };

    **Exactly one** `endpoint.accept()`. Every one of the 74 `accept()`
    call sites in `tests/` is likewise a single call — `grep -rn "\.accept()"
    tests/*.rs` returns no loop anywhere, and `Pair::establish` is what
    almost every story test uses. So the harness **cannot express** the
    lost-msg2 case: drop msg2 with `FlakyPolicy` and this fixture hangs on
    the dial and panics at `expect("the dial completed")`, which reads as a
    fixture bug rather than as the application obligation it actually is.
    Rule 13: *"when a whole class of fault is absent from the results,
    suspect the harness."* Ruling 253's acceptance evidence needs either a
    looping `establish_lossy` or a story-level test that accepts twice.

90. **`STORIES.md:51–63` (S1, the dial story)** — DEPENDS. Quoted in full,
    because the brief asks whether it says anything about loss: **it does
    not.**
    > ### S1 — a user can open a connection and close it *(maintainer's #1)*
    >
    > Dial a peer by static public key and address; get a live connection;
    > close it cleanly; both sides observe the close.
    >
    > - **Accepts:** `connect(addr, static)` returns a `Connecting` that
    >   resolves to a `Connection`. `close(code, reason)` resolves once the
    >   CLOSE frame is sealed. The peer surfaces
    >   `ConnectionLost::PeerClosed { code, reason }` with the same code and
    >   reason (truncated at `CLOSE_REASON_MAX`); our own side sees
    >   `ConnectionLost::LocallyClosed`.
    > - **Cost:** 4 DH (initiator side: `es`, `ss`, `ee`, `se`).
    > - **Anchor:** §16.1, §15.1, §15.2. **Paused clock:** yes.

    No loss, no retry, no second accept. S2 (`STORIES.md:65–74`) covers
    *no answer at all* and §5.5's retransmit schedule, but from the
    **initiator's** side only — its accept criterion is
    `ConnectError::TimedOut` at 90 s. **Neither story has a responder that
    must accept twice.** If 253 is to be acceptance-tested, the honest
    reading is that it needs a story (S34) or an explicit extension of S1,
    not a test hung off an existing one.

91. **`STORIES.md:142–145`** (section B header) — DEPENDS (the model)
    > The ladder is the point: **0 DH to see it, 1 to inspect it, 2 to prove
    > it, 4 to accept it.** Each stage is a place the application may stop.

    Frames the whole inbound section as *one* ladder climbed *once*. Nothing
    in S6–S10 contemplates climbing it again for the same peer.

92. **`STORIES.md:211–221` (S11)** — BENIGN
    > - **Accepts:** when an accept is refused because the live connection's
    > … parked `Intro` becomes acceptable on the next attempt.

    "On the next attempt" is the only story text that implies repetition,
    and it is scoped to the contested/`Stale` path.


## B6. Appendix B gains a §7.7 rekey obligation (ruling 252)

### Confirmation: Appendix B claims no rekey coverage today

93. **Appendix B (`SPEC.md:6967–7510`) contains zero occurrences of
    "rekey", "epoch", "ratchet" or "§7.7".** Measured, not argued:
    `awk 'NR>=6967 && NR<=7510' SPEC.md | grep -i "rekey\|epoch\|ratchet"`
    returns nothing. Its ten headings are Wire pins · Handshake and routing ·
    Frame layer · Streams, flow control, messages, datagrams · ACK, recovery,
    congestion · Liveness and amplification · CLOSE · The shell surface · The
    composability surface · Post-implementation validation obligations
    (`SPEC.md:6973, 6982, 7077, 7087, 7134, 7162, 7366, 7376, 7443, 7491`).
    **§7.7 is the only numbered subsection of §7 with no Appendix B
    obligation** — §7.2/§7.3/§7.4/§7.5 are all covered under "Liveness and
    amplification" (`:7162`) and §7.9 under the same. So there is no
    contradiction to sweep, only an absence. Ruling 252 is a pure addition.

94. **`SPEC.md:7162`** — the placement question. A §7.7 obligation is
    thematically a *session* obligation, not a liveness one, so it either
    joins "Liveness and amplification" (which already carries §7.2's replay
    window) or gets its own heading. Worth stating in the ruling, because
    Appendix B's headings are the index a test author navigates by.

### The §7.7 claims that would need pinning, and their current status

95. **`SPEC.md:3136–3138`** (§7.7) — **an unmet in-text test claim**,
    CONTRADICTS in the weak sense that the spec asserts a pin that does not
    exist
    > The
    > ChaCha20-Poly1305 vector, pinned by test:
    > `REKEY(0³²) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`.

    `grep -rn "25ce5d37" src tests` returns **nothing**. The spec says
    "pinned by test" and no such test exists in this crate. (It may be
    pinned inside hiss — but §7.7 is slither's spec and the sentence reads
    as a slither obligation.) This is precisely the kind of claim ruling 252
    should either discharge or re-scope; rule 11 says the artefact a claim
    is about must be opened, and this one was.

96. **`SPEC.md:3139–3141`** (§7.7) — untested behaviour
    > The receiver retains the current and immediately preceding epoch keys
    > (straggler tolerance: one epoch back); anything older is refused, its key
    > ratcheted away.

97. **`SPEC.md:3149–3170`** (§7.7, epoch death) — untested behaviour
    > **Implementations must not chase epochs.**

98. **`SPEC.md:3172–3186`** (§7.7, ruling 82) — DEPENDS, and the mechanism
    that makes 252 testable at all
    > **The epoch size is config-supplied for tests, `REKEY_EPOCH_MSGS`
    > otherwise.** … A configurable epoch therefore pins the
    > boundary *behaviour* and a separate constant test pins the *value* —
    > independently, which is the stronger arrangement

    Rule 11: the mechanism exists (`Config::with_epoch_size`,
    `src/config.rs:183`). **But the arrangement ruling 82 describes is only
    half-built.** `grep -rn "with_epoch_size" src tests` finds exactly three
    hits: the definition (`src/config.rs:183`), one config unit test
    asserting the setter stores the value (`src/config.rs:237`), and a
    rustdoc cross-reference (`src/core/endpoint/mod.rs:768`). **No test
    anywhere configures a small epoch and crosses a boundary.** So the
    "boundary *behaviour*" half of ruling 82's stronger arrangement has
    never been written, and §7.7 has **no behavioural coverage of any kind**
    — only `tests/spec_constants.rs:274–282` pinning the two constants.

99. **`STORIES.md:355–361` (S23)** — DEPENDS, an approved story with no test
    > ### S23 — a long-lived connection rekeys itself without the user noticing
    >
    > - **Accepts:** the per-direction epoch ratchet advances every 65 536
    >   messages with no handshake, no round trip and no application-visible
    >   event. There is no DH re-handshake; a new handshake from a live static
    >   means replacement (S3b), not rekey.
    > - **Anchor:** §7.7. **Paused clock:** yes.

    `CLAUDE.md` says *"a slice is done when its stories are paused-clock
    tests that pass"*. S23's acceptance criterion is behavioural ("advances
    … with no application-visible event") and no test asserts it.
    **STORIES.md does not claim coverage** — it is the criterion, not the
    evidence — so this is not a contradiction, but it is the strongest
    argument for ruling 252 and the ruling should cite it.

100. **`STORIES.md:88`** — BENIGN
     > installs the new one (the ratchet-only ruling: sessions:connections is

101. **`SPEC.md:531–535`** (§1.3's ratified-decisions table) — BENIGN, all
     still true; lists the four §7.7 invariants a new obligation would pin.

### Test-file name collision

102. **No collision.** `ls tests/` gives: `spec_compat.rs`,
     `spec_constants.rs`, `spec_errors.rs`, `spec_packet.rs`,
     `spec_shell.rs`, `spec_streams.rs`, `story_codec.rs`,
     `story_compat.rs`, `story_datagram.rs`, `story_dial.rs`,
     `story_keepalive.rs`, `story_lifecycle.rs`, `story_message.rs`,
     `story_mobility.rs`, `story_path.rs`, `story_reliability.rs`,
     `story_streams.rs`, `story_tower.rs`. **`tests/spec_rekey.rs` is free**,
     and a `tests/story_rekey.rs` (for S23) is free too.

     **Rule 15 applies in full, and this is a trap.** `Cargo.toml` carries
     **fifteen** explicit `[[test]]` stanzas (`Cargo.toml:220–302`), each
     with `required-features = ["test-util"]` or more, and the manifest
     comment at `:268–274` states the hazard verbatim:
     > cargo does not warn about a `[[test]]` whose file is
     > missing, it **refuses to parse the manifest**, so an implementer adding a
     > stanza for its partner's not-yet-written file commits a tree on which no
     > gate can run at all (working rule 15).

     There is **no `autotests = false`** (`Cargo.toml:271–274`, ruling 194),
     so a new `tests/spec_rekey.rs` is *also* auto-discovered **without** its
     `required-features` and the feature-less `cargo test` gate fails
     outright until its stanza lands. A rekey test needs `testutil`, so the
     stanza is mandatory and it is the **integrator's** to add — not either
     blind agent's.


## B7. Cross-cutting: 212(c) citations; driver debug_asserts

### Every `212(c)` citation in the repo (10 sites, 5 files)

`grep -rn "212(c)" SPEC.md .spec-v2-clean-slate/rulings.md src/` — complete:

| # | Site | Verbatim | Class |
|---|---|---|---|
| 103 | `SPEC.md:507` | "An earlier ruling (212(c)) lifted the challenge above the probe instead and was reversed" | BENIGN — §1.3's history of the closed flag; already says "reversed" |
| 104 | `SPEC.md:2456` | "challenge above the probe was ruled first (212(c)) and **reversed**" | BENIGN — same, in §7.3 |
| 105 | `.spec-v2-clean-slate/rulings.md:6204` | ruling 215's title | BENIGN |
| 106 | `.spec-v2-clean-slate/rulings.md:6206` | "**T1, blind, found that ruling 212(c) contradicts `SPEC.md` at T1's own base commit**" | BENIGN |
| 107 | `.spec-v2-clean-slate/rulings.md:6214` | "**What I ruled (212(c)):** rank `PATH_CHALLENGE`/`PATH_RESPONSE` above the contested probe" | BENIGN |
| 108 | `.spec-v2-clean-slate/rulings.md:6247` | "212(c)'s **other** half was right and is untouched" | BENIGN |
| 109 | **`src/core/connection/mod.rs:2056`** | "**[ruling 212(c)]** §7.3's ranks 2 and 3 — `PATH_RESPONSE` and `PATH_CHALLENGE`, immediately after CLOSE and **above** the contested probe." | **CONTRADICTS** — cites the reversed half as live law (see hit 48) |
| 110 | **`src/core/connection/mod.rs:2064`** | "which ruling 212(c) names as wrong as written" | DEPENDS — the surviving half, but the citation should be 215 |
| 111 | **`src/core/connection/mod.rs:2567`** | "a **pending** contested probe (**[ruling 212(c)]**)." | DEPENDS |
| 112 | **`src/core/connection/testfix.rs:794`** | "Ruling 212(c) is exactly such a statement, and it is unassertable through `pump_from`." | BENIGN — the claim is about per-pass ordering statements generally |
| 113 | **`src/core/connection/tests_path.rs:30`** | "**ruling 212(c) ranks them" (module header) | DEPENDS |
| 114 | **`src/core/connection/tests_path.rs:951, 971, 978, 979, 996, 1028, 1036, 1050`** | the reported-conflict block and two test docs (see hits 53, 57) | DEPENDS |
| 115 | **`src/core/connection/tests_contested.rs:685, 1092`** | "**[I1, ruling 212(c)]**" (see hits 54, 55) | DEPENDS |

**The pattern worth naming:** the two `SPEC.md` sites and all four
`rulings.md` sites say "reversed"; **every `src/` site does not**. Ruling
215 swept the documents and left the code citing the reversed ruling as
current law — the same one-artefact-at-a-time defect ruling 215 itself
diagnosed. When 251 lands, all eight `src/` sites need their citation
updated, and `mod.rs:2056` needs its *behaviour* changed, not only its
citation.

### `debug_assert` in `src/shell/driver.rs` — all six

`grep -n "debug_assert" src/shell/driver.rs`:

| Line | Assertion | Role |
|---|---|---|
| 116 · `:519–522` | `debug_assert!(matches!(event, ToEndpoint::Retired { .. }), "§16.4 defines exactly one connection→endpoint event")` | §16.4 event-set contract |
| 117 · `:757` | `debug_assert!(false, "ConnEvent::Established without an installed session");` | unreachable-arm guard |
| 118 · `:1008` | `debug_assert!(false, "the endpoint core queued an output outside a drain");` | §16.4 drain contract |
| 119 · `:1020` | `debug_assert!(false, "a connection core queued an output outside a drain");` | §16.4 drain contract |
| 120 · **`:1028`** | `debug_assert!(*announced > fired_at, "{PAST_DEADLINE}");` | **← the past-deadline guard (F1's spin detector)** |
| 121 · `:1195` | `debug_assert!(false, "§16.4: `accept()` returns an established connection");` | §16.4 contract |

`:969` is a *doc-comment* mention of `debug_assert!(deadline >= now)`, not a
call — it is the predicate `CONTRACT-7b.md` §4.2 specified and the driver
deliberately **rejected** as too strong.

**Why `:1028` matters to ruling 249 (B2), stated as a measurement rather
than an argument.** The guard is `announced > fired_at`, keyed on
`self.last_timeout`, which is set at `driver.rs:936` inside `handle_timeout`
and **cleared at `driver.rs:273` on every non-`Timeout` loop event**. Under
249 a suppressed `Pto` is re-armed only when §7.3's budget grows, and the
budget grows only on an authenticated window-fresh **receive** — i.e. on an
`Event::Received` pass, where `last_timeout` is already `None`. So a
re-armed-in-the-past `Pto` deadline cannot trip this assertion. The strict
`>` (rather than `>=`) means even a re-arm landing exactly on `fired_at`
would trip it — which is the intended catch, and 249 must not produce one.
**Verified by reading `driver.rs:255–297, 929–945, 1013–1031`; not
inferred.**


---

## NON-BENIGN HITS (summary)

**68 non-benign hits** (CONTRADICTS or DEPENDS) across 6 changes; the
remainder of the sweep's findings are BENIGN and listed in place. Ranked
within each change; the eight in **bold** are the ones that ship a
self-contradicting document, a red gate, or a live conformance break if
missed.

### B1 — `PTO_BACKOFF_CAP` 2⁶ → 2³ (ruling 250) — 15
1 `SPEC.md:4516` C · 2 **`SPEC.md:4518–4521` C** (the "overflow guard, not a
death sentence" rationale — the only prose arguing *for* a large cap) ·
3 `SPEC.md:4540` C · 4 `SPEC.md:4702–4703` D (the only cap-derived magnitude
in the spec) · 5 `SPEC.md:7540` C · 6 `SPEC.md:7576` C · 7 `SPEC.md:7586–7591` D ·
8 `rulings.md:1025–1029` D · 9 `rulings.md:1046–1057` D ·
11 `src/constants.rs:383–384` C · 12 `src/constants.rs:607` D (compile-time,
turns red) · 13 `src/constants.rs:621–624` D · 14 `src/core/connection/recovery.rs:175–184` D
(`PTO_MAX_EXPONENT == 6` — **will not compile**) ·
16 `src/core/connection/tests_recovery.rs:1108–1145` C (test *name* encodes 64) ·
17 `tests/spec_constants.rs:578, 584–585` C

### B2 — `Pto` disarmed under the budget (ruling 249) — 11
21 `SPEC.md:4528–4530` D (rule 8) · 22 `SPEC.md:4560–4563` D ·
**24 `SPEC.md:4633` C — the roam table's "no re-arm, no cancel", inside a
list ruling 173 ratified as exhaustive, four lines under the row that zeroes
the budget** · 25 `SPEC.md:4587–4589` C · 25b `SPEC.md:4607–4609` D ·
26 `SPEC.md:5975–5978` D (§16.5's timer contract; also the "independent"
claim) · **29 `rulings.md:3500–3502` C — ruling 139(a)'s *entire*
slither-specific rationale is the case 249 removes** ·
31 `src/core/connection/recovery.rs:359–366` D · 32 `recovery.rs:376–382` C
(the `None` **iff** the map is empty) · 33 `mod.rs:1496–1498` D ·
34 `mod.rs:689–694` D

### B3 — pump-time arithmetic + coalescing (ruling 251) — 17
37 `SPEC.md:2418–2423` C · 38 `SPEC.md:2448–2451` C · 39 `SPEC.md:2462–2464` C
("and always does") · 40 `SPEC.md:2273` D ·
**41 `SPEC.md:4758–4760` C and 42 `SPEC.md:4953` C — both still send the
reader to an open flag ruling 215 closed** · 43 `rulings.md:6204` D ·
44 `rulings.md:6232–6237` D (the three-resolutions sentence 251 must address) ·
45 `rulings.md:6243–6246` D ·
**48 `src/core/connection/mod.rs:2055–2069` C — the pump implements 212(c),
which ruling 215 reversed; this is a conformance break against the current
spec, today** · 49 `mod.rs:2071–2085` D · 50 `mod.rs:2565–2578` D (+ the
unanswered cwnd-gate question) · 51 `mod.rs:2546–2551` D ·
53 `tests_path.rs:1062` C (name) · **54 `tests_contested.rs:684–692` D — goes
red under coalescing** · 55 `tests_contested.rs:1092–1098` D ·
57 `tests_path.rs:951–985` D

### B4 — reassembly small-to-large (ruling 254) — 10
61 `SPEC.md:4222–4227` D · 63 `SPEC.md:4245–4251` D ("never allocating ahead
of arrival") · 64 `SPEC.md:4254–4256` D · 68 `rulings.md:2143–2152` D ·
69b `rulings.md:6121–6127` D (213(c)'s "not linear in them") ·
70 `src/core/connection/recv.rs:469–489` D (the "left alone" sentence) ·
71 `recv.rs:549–579` D · **74 `recv.rs:693–703` D — `assert_eq!(half.capacity(),
2_000)` goes red under any growth-amortised merge** ·
77 `tests_reassembly.rs:38–61` D (the F3 separator's mechanism is voided) ·
79 `tests_reassembly.rs:245–256` D

### B5 — `accept()` obligation (ruling 253) — 6
82 `SPEC.md:1675–1676` D (rule 8: scoped to *dialling* applications only) ·
87 `src/lib.rs:9–11` D (the crate root teaches an establish-once model) · 88 `src/shell/endpoint.rs:75–115` D ·
**89 `src/testutil/mod.rs:1217–1243` D — `Pair::establish` accepts exactly
once, and so does every one of the 74 `accept()` sites in `tests/`; the
harness cannot express the lost-msg2 case (working rule 13)** ·
90 `STORIES.md:51–63` D (S1 says nothing about loss) · 91 `STORIES.md:142–145` D

### B6 — Appendix B §7.7 obligation (ruling 252) — 3
**95 `SPEC.md:3136–3138` — the spec says the `REKEY(0³²)` vector is "pinned
by test" and `grep -rn "25ce5d37" src tests` returns nothing** ·
98 `SPEC.md:3172–3186` D (ruling 82's "stronger arrangement" is half-built:
`with_epoch_size` is never used to cross a boundary) · 99 `STORIES.md:355–361` D
(S23 has no behavioural test)

### B7 — citations — 6
**109 `src/core/connection/mod.rs:2056`** · 110 `mod.rs:2064` ·
111 `mod.rs:2567` · 113 `tests_path.rs:30` · 114 `tests_path.rs:951/971/978/
979/996/1028/1036/1050` · 115 `tests_contested.rs:685/1092`
— every `212(c)` citation in `src/` presents the reversed half as live law,
while every citation in `SPEC.md` and `rulings.md` says "reversed".

---

## CONFLICTS (rule 3)

Statements that conflict **with each other**, independent of rulings 249–254.
Reported, not resolved.

### C1 — `rulings.md:1027` cites §13.5 for a sentence that is in §14.4, and always has been
> **Settled by reading, no ruling needed.** `PTO_BACKOFF_CAP` = 2⁶ is the
> **multiplier (64), not the exponent**: §13.3 caps `2^pto_count`, and
> **§13.5 says "2⁶× too long"**.

§13.5 is *"Frames, never packets"* (`SPEC.md:4572`) and contains no such
phrase. The sentence is in **§14.4** (`SPEC.md:4702`). **Dated, not
assumed** (ruling 213's method): at the original ratification commit
`2274981`, "2⁶× too long" was at `SPEC.md:3537`, inside §14.4 (3526–3546),
and §13.5 was already "Frames, never packets" at 3442. The citation has
never resolved. Half of the two-part argument that settled
`PTO_BACKOFF_CAP`'s reading points at the wrong section — rule 11's *"a
citation is a claim about the cited text"*, in the passage ruling 63 then
built on.

### C2 — `SPEC.md` contradicts itself three ways about whether §7.3's flag is open
- `SPEC.md:493–495`: *"`[FLAGGED FOR RULING]` **now appears nowhere in this
  document, and 0 is again the expected answer** — §7.3's entry is
  `[RATIFIED … ruling 215]`."*
- `SPEC.md:2440–2441`: *"**[RATIFIED 2026/08/16 — ruling 215, closing this
  section's one open flag. §1.3's expected flag count returns to zero.]**"*
- `SPEC.md:4758–4760` (§14.5): *"§7.3 carries a **flagged, unresolved**
  interaction between the probe's rank and the challenge's — read the order
  there, not here."*
- `SPEC.md:4953` (§15.4): *"§7.3 also carries a **flagged, unresolved**
  interaction between this rank and ruling 208's `PATH_CHALLENGE`."*

Two sections send the reader to an open flag two other places say is closed.
Ruling 215 wrote *"§14.5 needs no change, because the ranks it describes
were right all along"* — the ranks were; the flag reference in the same
sentence was not. This is rule 4(a) (*read the other clauses of the sentence
you are correcting*) failing on the ruling that named the identical process
defect one paragraph earlier.

### C3 — the send pump contradicts ratified §7.3 today
`SPEC.md:2397–2408` ranks **1 CLOSE · 2 contested probe · 3 `PATH_RESPONSE`
· 4 `PATH_CHALLENGE`**, and `rulings.md:6242–6244` says those ranks *"stand
exactly as written"*.

`src/core/connection/mod.rs:2056` says:
> **[ruling 212(c)]** §7.3's ranks 2 and 3 — `PATH_RESPONSE` and
> `PATH_CHALLENGE`, immediately after CLOSE and **above** the contested probe.

and the behaviour follows the comment: `pump_path_frames` (`mod.rs:2577`)
emits a **separate datagram** of path frames before `pump_contested_probe`
runs. This is not a stale comment — it is the ranking ruling 215 reversed,
still executing. `CLAUDE.md`'s first hard rule is that the code must match
the spec.

### C4 — the roam seam already contradicts itself, before ruling 249
Three statements in `SPEC.md`, all currently ratified:
- `:4629` — *"amplification byte counters (sent, received) | **reset to
  zero**, and the address becomes unvalidated"*
- `:4568–4570` — *"Probes are **not** exempt from §7.3's anti-amplification
  budget on an unvalidated address."*
- `:4633` — *"PTO / loss detection | **undisturbed** — timers continue, no
  re-arm, no cancel"*

Immediately after a roam the budget is zero, so the PTO timer is armed and
firing while **no probe can leave**. "Undisturbed" is true of the timer and
false of the probing. §13.4 (`:4560–4563`) acknowledges the resulting
behaviour — *"a budget with no room … emits nothing, and the session dies at
`DEAD_TIMEOUT`"* — but §13.6's table asserts nothing is disturbed. Ruling
249 resolves this; the conflict predates it and should be named as the
reason 249 exists rather than as a consequence of it.

### C5 — `tests_path.rs` says both "unresolved" and "resolved" about the same conflict
- `:952` — *"⚠ **CONFLICT, REPORTED AND NOT RESOLVED (working rule 3).**"*,
  followed by 33 lines arguing 212(c) against §7.3.
- `:1049–1051`, 100 lines later — *"**[Integrator, ruling 215 — resolved,
  and this test does NOT invert.]**"*

One file, two verdicts, no cross-reference between them. A reader who stops
at the header block takes 212(c) as unresolved law; the tests below it were
written that way.

### C6 — §7.7 asserts a test that does not exist
`SPEC.md:3136–3138`:
> The ChaCha20-Poly1305 vector, **pinned by test**:
> `REKEY(0³²) = 25ce5d37df19f3783185f2ffd5ab17fa3397c212f02d62fb1733e0b875b74c58`.

`grep -rn "25ce5d37" src tests` returns nothing. Either the pin is hiss's
and the sentence should say so, or the obligation is slither's and is
undischarged. This is the same shape as C1: a claim about an artefact,
refuted by opening it.

### An asymmetry, reported as an observation rather than a conflict
§16.5 (`:5959–5962`) refuses to arm the **`Contested`** deadline for output
§7.3's budget will not admit — *"armed at the probe's **transmission** —
never at the mark, which may wait on §7.3's budget"* — while §13.3
(`:4528–4530`) arms **`Pto`** with no reference to the budget at all. Two
timers, one budget, opposite treatment, and neither section mentions the
other. Ruling 249 harmonises them; §16.4's `Contested` passage
(`:5849–5862`) is the precedent it should cite.

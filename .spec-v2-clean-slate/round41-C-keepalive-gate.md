# Round 41 · Item C — the keepalive announce-gate is unspecified

Base commit: `94dab20`. Read-only investigation. All citations `file:line`.

## 0. Base verification

## 1. `keepalive_can_leave()` — what it gates

## 2. The `tests_livelock.rs` pins

## 3. Spec homes — §7.5, §7.3, §13.3, §16.4/§16.5

## 4. The soundness question (ruling 249's argument, applied to keepalives)

## 5. Rule-4 sweep — contradicting prose

## 6. Candidate clauses

## 7. VERDICT

`git rev-parse HEAD` = `94dab200a172a5b62b4d28711962574d61a9d3c8` — matches the briefed base.
`git status --porcelain` empty (clean tree; nothing read from an uncommitted state, rule 16).

### Ruling 249's recorded deferral (the task's anchor) — `rulings.md:7215` (heading), body

Quoted verbatim from the **Scope guard** paragraph, `.spec-v2-clean-slate/rulings.md:7307–7316`:

> **Scope guard:** this rules `Pto`, the measured case. The keepalive was
> checked and is already safe: slice 7b's F1 fix gates both keepalive
> announcements on the budget (`keepalive_can_leave()`,
> `mod.rs:2804–2807`) and `tests_livelock.rs` pins that no beacon deadline
> is armed at zero room — F1's spin is unreachable rather than survivable.
> **The residual gap is documentary and is recorded here, deferred: the spec
> is silent about the keepalive announce-gate the code implements (a grep
> for a keepalive-budget clause in `SPEC.md` finds only §7.3's 90 B
> arithmetic) — round-41 material, not a spin.**

So the deferral is explicit, the gap is stated to be **documentary only**, and
249 asserts the code side is **already safe**. Task 4 re-checks that assertion
rather than inheriting it (rule 11: a citation is a claim about the cited text;
249's safety claim is a claim about code, so the code is opened below).

**Line drift, noted not faulted:** 249 cites `keepalive_can_leave()` at
`mod.rs:2804–2807`; at base `94dab20` it is `src/core/connection/mod.rs:2956`.
Ruling 250 and the rest of round 40 landed between. The *predicate* is the same
one; only the line moved.

---

# 1. `keepalive_can_leave()` — what it actually gates

**Definition — `src/core/connection/mod.rs:2956–2959`:**

```rust
fn keepalive_can_leave(&self) -> bool {
    let size = (constants::DATA_HEADER_LEN + constants::AEAD_TAG_LEN) as u64;
    !self.contested.is_pending() && self.amplification.admits(size)
}
```

Doc comment `mod.rs:2949–2955`: *"`transmit_keepalive`'s guard and
`sync_liveness_timer`'s arming condition are the same question, and a build
that states it twice is the build that drifts"*.

## 1.1 The two call sites

| site | file:line | role |
|---|---|---|
| `sync_liveness_timer` | `mod.rs:2928` (`let can_leave = self.keepalive_can_leave();`) | **announcement** gate |
| `transmit_keepalive` | `mod.rs:3019` (`if !self.keepalive_can_leave() { return; }`) | **transmission** guard |

## 1.2 Which keepalives the announcement gate covers — **both**

`sync_liveness_timer`, `mod.rs:2934–2946`:

```rust
let passive = clocks.filter(|l| l.owes_passive_keepalive() && can_leave);
self.timers.set(TimerKind::Keepalive,
    passive.map(|l| l.last_send() + constants::KEEPALIVE_TIMEOUT));

let beacon = self.persistent_keepalive.filter(|_| can_leave);
self.timers.set(TimerKind::PersistentKeepalive,
    clocks.zip(beacon).map(|(liveness, interval)| liveness.last_send() + interval));
```

So `TimerKind::Keepalive` (§7.5 passive) **and**
`TimerKind::PersistentKeepalive` (§7.5 beacon) are both suppressed.
`TimerKind::Liveness` is set **unconditionally** at `mod.rs:2926–2927` — the
death clock is deliberately not gated (`mod.rs:2915–2920`: *"suppressing it
too would turn a spinning connection into an immortal one, which is the
collapse ruling 182's beacon proof warns about and is strictly worse than the
spin"*).

## 1.3 Announcement vs state

Same shape as ruling 249's Pto gate: **the gate is on the announcement, not
on the state.** `Liveness`'s `last_send` / `received_since_marking_send` /
`armed` (`session.rs:253–279`) are untouched by the suppression; only
`Timers::set` sees `None`.

## 1.4 The comparison ruling 249 asked for — **and the discrepancy**

| | `Pto` (ruling 249) | keepalives (slice 7b F1) |
|---|---|---|
| anchor | `Recovery::pto_deadline()` — `anchor + interval × 2^min(pto_count, PTO_MAX_EXPONENT)` | `Liveness::last_send() + KEEPALIVE_TIMEOUT` / `+ interval` |
| arming site | `sync_recovery_timers`, `mod.rs:1560–1566` | `sync_liveness_timer`, `mod.rs:2934–2946` |
| shape | `pto_deadline().filter(\|_\| self.probe_can_leave())` | `clocks.filter(\|l\| … && can_leave)` / `persistent_keepalive.filter(\|_\| can_leave)` |
| predicate | `probe_can_leave` (`mod.rs:1598–1601`) = `amplification.admits(39)` — **one conjunct** | `keepalive_can_leave` (`mod.rs:2956–2959`) = `!contested.is_pending() && amplification.admits(30)` — **two conjuncts** |
| size asked | `DATA_HEADER_LEN + 1 + 8 + AEAD_TAG_LEN` = 39 (§13.4's challenge datagram) | `DATA_HEADER_LEN + AEAD_TAG_LEN` = 30 (§3.4's empty plaintext) |
| transmission-side twin | `pump_contested_probe`'s budget stop (`mod.rs:1584–1587` doc) — a *different* function | `transmit_keepalive`'s guard — **the same** function, by design |

**Finding C-1 (reportable, rule 11 / rule 4(b)).** Ruling 249's scope guard
says the F1 fix *"gates both keepalive announcements **on the budget**"*
(`rulings.md:7308–7311`). That is **half the predicate**. The shipped gate is
`!contested.is_pending() && amplification.admits(30)`; the first conjunct is
§7.5's contested mark, not §7.3's budget at all. The distinction is
load-bearing and the test file is built entirely around it — the whole
one-byte `room == 30` / `room == 31` calibration
(`tests_livelock.rs:427–441`, `:529–541`) exists precisely because the two
disjuncts are separable. **A clause drafted from ruling 249's sentence alone
would specify a narrower gate than the code implements.** This is not a
conflict in the code; it is an imprecision in the ruling's characterisation
that a round-41 clause must not inherit.

---

# 2. The `tests_livelock.rs` pins (10 tests, `src/core/connection/tests_livelock.rs`)

Module header `:1–69` states the file's premise: *"A livelock is not a wrong
value — it is the same correct value forever"* (`:20–21`), so every test
asserts from the **separating** side (working rule 9), via three shapes
tabulated at `:25–29`: non-retrospection, termination, monotone advance.

| # | test (line) | what it separates | would a build **without** the gate fail it? |
|---|---|---|---|
| 1 | `beacon_refused_by_the_budget_does_not_re_arm_in_the_past` (`:301`) | `room == 0`, **no** contested mark — the **amplification** conjunct alone | **Yes.** Ungated `sync_liveness_timer` announces `last_send + interval` ≤ `now`; `assert_not_retrospective` (`:256`) fires. Also fails a partial fix that consults `contested` only (`:294–296`). |
| 2 | `a_refused_beacon_does_not_re_deliver_itself_at_the_same_instant` (`:388`) | monotone advance at the same instant | **Yes** — and additionally kills a *clamping* fix (`max(d, now)`), which #1 alone would not (`:378–386`). |
| 3 | `beacon_blocked_by_a_pending_mark_does_not_re_arm_in_the_past` (`:443`) | `room == 30` **exactly**: budget admits the 30-byte keepalive, refuses the 31-byte probe — the **contested** conjunct alone | **Yes.** This is the test a budget-only gate fails (`:436–441`). It is the direct pin on the conjunct ruling 249's sentence omits. |
| 4 | `one_more_byte_of_room_releases_the_probe_and_the_isolation_with_it` (`:541`) | the calibration guard at `room == 31`: the probe **is** admitted and the mark arms | Not a gate test — it is the anti-rot pin that keeps #3 isolating `contested` rather than silently becoming a second budget test (`:529–539`). Working rule 9's *"the bound is only a separation while the neighbouring value falls on the other side"*. |
| 5 | `a_connection_holding_its_beacon_still_reaches_a_fixed_point` (`:625`) | **termination** under the budget hold, via `drive_like_the_shell` (`:587`) | **Yes** — panics on the first step where `next == now` (`:577–580`). Its doc at `:617–620` already records ruling 249: *"a build that suppresses correctly announces no `Pto` at all while the budget is closed (§13.3, ruling 249 — before that ruling it walked the PTO backoff here)"*. |
| 6 | `a_connection_holding_its_beacon_behind_a_mark_still_reaches_a_fixed_point` (`:654`) | termination under the **contested** hold | **Yes**, same mechanism, other conjunct. |
| 7 | `an_admissible_beacon_still_fires_and_advances` (`:681`) | **over-suppression control** — validated address, no mark: the beacon must fire, seal 30 bytes, re-arm one interval on | Inverse direction: fails a build that "fixes" F1 by never arming the beacon (`:672–677`). |
| 8 | `an_admissible_passive_keepalive_fires_and_then_disarms` (`:746`) | over-suppression control for `TimerKind::Keepalive` specifically | Inverse direction. Its doc (`:727–732`) is explicit that the passive timer has **no held-state test**, so this control is the only thing catching a `keepalive_can_leave` wired into the passive arm *with the wrong sense*. |
| 9 | `a_freshly_installed_core_announces_its_death_clock_and_not_none` (`:839`) | `Timeout(None)` is not a free pass — the install instant, §7.4's arming event | Blocks any suppression broad enough to reach `TimerKind::Liveness` (`:834–837`). |
| 10 | `the_beacon_returns_when_the_hold_lifts` (`:954`) | **re-arming** — a suppressed beacon comes back when the hold lifts | **Yes**, and this is the one that pins ruling 249's re-arm argument for the keepalive. Its doc (`:936–946`) records that the blind author and the implementer closed F1 differently and that *"an armed-in-the-future beacon is self-healing; a suppressed one is only as good as whatever re-arms it"* (ruling 220). |

Plus the shared companion assertion `assert_death_clock_armed` (`:278`),
required by `CONTRACT-7b.md` §4.3 and called at three instants, so that
`fn deadline() -> None` cannot pass the file (`:269–276`).

**Coverage gap the file itself declares:** `the_passive_form` (`:873–928`, a
doc-only `fn the_passive_form() {}`) — there is **no test of
`TimerKind::Keepalive` in its held state**. See §4 below; this is the crux of
the soundness question.

---

# 3. The spec homes

## 3.1 §13.3 — the Pto twin, as it now reads (`SPEC.md:4585`; the amendment at `:4616–4638`)

The normative sentence, `SPEC.md:4616–4620`:

> **The `Pto` timer is armed only while at least one ack-eliciting packet
> is in the sent map** (RFC 9002 §6.2.1) **and while §7.3's amplification
> budget admits a probe datagram** **[AMENDED 2026/08/17 — ruling 249]**;
> when the map empties it is disarmed, and when the `Loss` timer is armed it
> takes precedence (§16.5).

and the announce-gate paragraph, `SPEC.md:4629–4638`:

> The second precondition gates the **announcement**, not the state: sent
> map, `pto_count` and anchor are untouched while the budget is closed, the
> connection's `Timeout` falls to the next armed timer — `Liveness` at the
> latest — and the deadline is announced again at the authenticated,
> window-fresh receive that refunds the budget (§7.2, §7.3; every receive
> recomputes the `Timeout`, so no dedicated re-arm machinery exists). This
> is §16.4's `Contested` principle — a deadline is never announced for
> output that cannot leave — applied to `Pto`.

That is the exact template a keepalive clause should follow.

## 3.2 §7.5 — where the keepalive arming rules actually live (`SPEC.md:2671`)

The two rules are stated as *firing* rules, with no arming/announcement
language and **no budget mention**:

* passive, `SPEC.md:2681–2685`: *"a side that has received since its last
  **marking** send, and has not made a **marking** send for
  `KEEPALIVE_TIMEOUT`, sends a keepalive"* (amended by ruling 182);
* beacon, `SPEC.md:2705–2708`: *"it fires when no marking send has occurred
  for the configured interval, and re-arms from every marking send"*.

Budget prose *does* appear in §7.5 — but only for the **contested probe**,
`SPEC.md:3105–3113`: *"Like every other exempt class the probe remains bound
by §7.3's anti-amplification budget at an unvalidated address"* and *"a probe
that §7.3's budget will not yet admit leaves the mark **pending** rather than
failed"*. Nothing there reaches the keepalive.

**So: §7.5 is silent, confirming ruling 249's deferral.**

## 3.3 §7.3 — what *is* already normative (`SPEC.md:2200`, rank list `:2422–2432`)

§7.3 already binds keepalive **transmission**:

* `SPEC.md:2205` names keepalives among the cwnd-exempt classes that the
  budget must still bind;
* `SPEC.md:2419–2421`: *"The budget binds all output and cannot be waived,
  so when it admits less than is owed, *something* must yield, and the order
  is normative"*;
* rank **7** is *"Keepalives — passive and persistent (§7.5)"* (`:2430`).

So `transmit_keepalive`'s **guard** (`mod.rs:3019`) is already covered by
ratified text. What is unspecified is only the **announcement** half —
exactly the half ruling 249 had to add for `Pto`.

§7.3's 90 B floor, which the whole soundness argument turns on, is at
`SPEC.md:2297–2298` (*"3× the smallest packet that can arm the budget —
§7.5's 30-byte keepalive, 90 B"*) and `:2445–2446`.

## 3.4 §16.5 — the timer paragraph, and a **rule-8 hit** (`SPEC.md:6076–6104`)

Ruling 249 amended this paragraph. It now reads, `SPEC.md:6093–6100`:

> `Pto` is armed only while an ack-eliciting packet is in the sent map **and
> §7.3's budget admits a probe** (§13.3, ruling 249) — the probe is
> ack-eliciting and is in that map (§13.5), so `Pto` and `Contested` can be
> armed together; they are not independent in one respect: **both wait on the
> same budget predicate**, `Contested` at the probe's transmission (§16.4)
> and `Pto` at the announcement; …

**Finding C-2 — the §16.5 sentence is now *false as an enumeration*.**
"both wait on the same budget predicate" ranges over `{Pto, Contested}`. In
the shipped code **four** of the eight named timers wait on a budget
predicate: `Pto` (`mod.rs:1563`), `Contested` (transmission), `Keepalive`
and `PersistentKeepalive` (`mod.rs:2934`, `:2940`). This is working rule 8's
signature — *a stated construction with an unstated or contradicted scope* —
and it was **introduced by ruling 249 itself**, one round after the keepalive
gate shipped. It is the strongest argument that the clause belongs at least
partly in §16.5.

Note also: this paragraph gives arming rules for `Contested`, `Pto` and
`Liveness`, and **none** for `Keepalive` / `PersistentKeepalive` — they are
named in the table at `:6076–6078` and nowhere else. Their arming rule lives
only in §7.5.

## 3.5 §16.4 — the principle the clause would cite (`SPEC.md:5969–5977`)

> a probe §7.3's amplification budget will not yet admit leaves the mark
> *pending* … and **an event fired at the mark would announce a countdown
> that is not running**.

This is the ratified precedent ruling 249 invoked (`SPEC.md:4636–4638`), and
it transfers to the keepalive without modification. No amendment needed here
— §16.4 is about the `Contested` **event**, not the keepalive timer.

## 3.6 §13.6 — the exhaustive roam table is missing a row (`SPEC.md:4738–4751`)

The table is declared exhaustive by ruling 173 (`SPEC.md:4721–4724`: *"a list
read as exhaustive had better be one"*). Ruling 249 **reworded the PTO row in
place** (`:4744`) to say the `Pto` announcement is budget-gated and therefore
suppressed from the roam.

**Finding C-3.** A roam zeroes the budget (`:4741`), which in the shipped code
also suppresses **both keepalive announcements** — a per-connection outcome of
the roam seam that the exhaustive table does not list. If §13.3/§16.5 gain a
keepalive announce-gate clause, §13.6 inherits the same duty ruling 249
discharged for `Pto`. Ruling 249's own precedent says the row is *reworded in
place, none added*; here there is no existing keepalive row to reword, so this
is genuinely an added row — which the ruling-173 table structure permits (it
is a list of per-connection outcomes, not a fixed schema).

---

# 4. The soundness question

Ruling 249's answer for `Pto` had two halves. **(A) Re-arm:** *"the budget
grows only on an authenticated, window-fresh receive, and every receive
already recomputes the `Timeout`."* **(B) Backstop:** *"the connection's
`Timeout` falls to the next armed timer — `Liveness` at the latest — and the
session still dies at `DEAD_TIMEOUT`."* Both halves are checked below against
the code, not inherited.

## 4.1 (A) Re-arm — **holds, and by a shorter path than `Pto`'s**

The budget conjunct:

* `Amplification::on_recv` is the only growth path (`mobility.rs:196–201`),
  and it is called at exactly one site — `mod.rs:633`, inside the
  window-fresh arm (ruling 169's comment, `:611–617`).
* **`sync_liveness_timer()` is called on the receive path itself**,
  `mod.rs:637` — one line after the credit, before `apply_live`. So both
  keepalive announcements are recomputed on the one event that can lift the
  budget hold. (`Pto`'s re-arm goes the longer way: `apply_live` → `pump` →
  `pump_inner` → `sync_recovery_timers`, `mod.rs:2093–2096`.)
* The roam arms with the triggering datagram's own credit —
  `Amplification::arm(challenge, datagram_len)`, `mod.rs:1354`.

The contested conjunct: `Contested::Pending` → `Armed` happens at the probe's
transmission (`pump_contested_probe`, `mod.rs:3197`), and the mark also clears
on an ACK covering its floor (§7.5, `SPEC.md:3121–3131`). Both are reached
from a receive, and `pump_contested_probe` runs at rank 2 on every pump
(`mod.rs:2117–2124`).

There are **10** `sync_liveness_timer()` call sites (`mod.rs:491, 637, 1795,
2040, 2558, 2763, 2843, 2993, 3046, 3271`), i.e. it is re-derived after every
send and after every receive. `the_beacon_returns_when_the_hold_lifts`
(`tests_livelock.rs:954`) pins the re-arm empirically.

**Verdict on (A): sound, same argument as ruling 249, one step shorter.**

## 4.2 (B) Backstop — **DOES NOT TRANSFER. This is the hole.**

`Pto`'s backstop is *"`Liveness` at the latest"*. `Liveness` is announced from
`Liveness::deadline()` (`session.rs:309–312`):

```rust
pub(crate) fn deadline(&self) -> Option<Instant> {
    self.armed.then(|| self.last_authenticated_recv + constants::DEAD_TIMEOUT)
}
```

`armed` is **cleared by every authenticated, window-fresh receive**
(`on_authenticated_fresh_recv`, `session.rs:336–344`) and set again only by a
**marking or ack-eliciting** send (`on_send`, `session.rs:320–333`; §7.4,
`SPEC.md:2588–2596`).

And `received_since_marking_send` — the passive keepalive's *entire*
predicate (`session.rs:365–367`, ruling 195) — is set by **the same call**.

> **Therefore: the passive keepalive is owed precisely in the window where
> the death clock is disarmed.** The one timer that is supposed to backstop
> the suppression is `None` in exactly the state the suppression is about.

The shipped fix's own soundness sentence asserts the opposite,
`mod.rs:2914–2920`:

> *"[`TimerKind::Liveness`] is deliberately **not** suppressed. The death
> clock keeps whatever `Liveness::deadline()` returns; suppressing it too
> would turn a spinning connection into an immortal one…"*

and `mod.rs:2911–2913`: *"the resulting state is a connection that announces
`Timeout(None)` and parks: **quiet, not immortal**."* Both are true when
`armed == true`. Neither is a statement about `armed == false`, and
`deadline()` returning `None` there is not suppression — it is §7.4's rule
working as designed. **Working rule 8's exact shape: a stated construction
(`Liveness` is never suppressed) with an unstated scope (it is only *armed*
after an arming send).**

### 4.2.1 The state, and how to reach it

The blind test author already flagged this and argued it was not
constructible — `the_passive_form`, `tests_livelock.rs:873–928`. Its
obstruction (`:884–891`):

> The debt is set **only** by an authenticated fresh receive … The same
> datagram credits §7.3's budget by `AMPLIFICATION_FACTOR × len`, and the
> smallest packet a peer can send is §3.4's empty plaintext at 30 bytes. So
> **every** receive that sets the debt also raises the room by at least
> `3 × 30 = 90` bytes.

That first step is **exactly right** and I verified it: with the invariant
`sent <= 3 × recv` (`mobility.rs:159–166`), `admits(30)` after a credit of
`L >= 30` reduces to `30 <= 3L`. So `keepalive_can_leave()`'s budget conjunct
is **true at the instant the debt is set**. Good.

The author's *second* step is where it breaks (`:895–902`):

> A pure ACK is the only member that is also non-ack-eliciting, and it costs
> **~35 bytes** against the ≥ 90 its own trigger credited: the room grows
> monotonically.

and the author flagged its own uncertainty (`:915–917`): *"building it needs
assumptions about `Packing`'s budget clamp that a blind author would be
guessing at."*

**A pure ACK does not cost ~35 bytes. It is sized to the room.**

* `Connection::packing()` clamps the plaintext capacity to
  `amplification.room() − 30` (`mod.rs:2137–2149`, ruling 203/207(c));
* `ack::derive(window, delay, room)` truncates newest-first *at that room*
  (`ack.rs:163–171`, `:200–215`), up to `MAX_ACK_RANGES = 64` pairs
  (`src/constants.rs:356`);
* `transmit_pure_ack` (`mod.rs:2805–2844`) then seals it `seal_quiet(now,
  &plaintext, **false**)` — **neither marking nor ack-eliciting** (§7.4,
  `SPEC.md:2641–2643`: *"a pure ACK is neither marking nor ack-eliciting, so
  it neither defers nor arms"*), charges `amplification.on_sent(size)`
  (`:2841`), and calls `sync_liveness_timer()` (`:2843`).

So a sufficiently fragmented replay window (which a roam does **not** reset —
§13.6 row, `SPEC.md:4749`) yields an ACK that drives `room` to **0** in one
non-arming packet. At `room == 90` the ACK needs ~60 frame bytes ≈ 26 range
pairs; §12.2 explicitly reasons about *"the 2048-bit alternating worst
case"*, so this is inside the modelled regime, not an exotic corner.

**Reaching it — the construction, stated so it can be measured:**

1. Established connection on a **validated** address; enough loss/reordering
   that §7.2's window holds ≳ 26 gaps; `bytes_in_flight` non-trivial.
2. Peer roams: one small **ack-eliciting** authenticated packet of `L` bytes
   from a new source. `commit_roam` (`mod.rs:1332`) re-arms the budget with
   credit `L` (`:1354`) → `room = 3L` (≈ 90–93); the congestion controller
   resets with pre-roam flight fenced (§14.6) while `bytes_in_flight` is
   kept, so §14.5's gate is closed (§13.6 says so in terms,
   `SPEC.md:4715–4719`).
3. The receive sets `armed = false` **and** the passive debt
   (`session.rs:336–344`).
4. Pump: rank 2 no contested mark; the owed ACK (stage 1) coalesces with the
   owed `PATH_CHALLENGE` (stage 2) → the packet is ack-eliciting →
   **§14.5's gate refuses it** (`mod.rs:2483`; the challenge is *not*
   cwnd-exempt — `mod.rs:2387–2394` says so and reads §14.5's list as closed
   under rule 8) → the code falls to *"A pure ACK is not gated. If one was
   owed it still goes out, alone"* (`mod.rs:2496–2501`).
5. `transmit_pure_ack` emits an ACK sized to the whole room → `room = 0`,
   nothing armed, nothing marked.
6. `sync_liveness_timer` (`:2843`): `Liveness = None` (`armed == false`),
   `Keepalive = None` (`admits(30)` false), `PersistentKeepalive = None`.
   `sync_recovery_timers`: `Pto = None` (`probe_can_leave` false,
   `mod.rs:1563`). `AckDelay` disarmed at `:2842`. `Loss` drains to `None`
   once the kept map is declared lost (retransmissions are held by the
   budget).

**Terminal state: `Timeout(None)` with no timer armed at all, and no
authenticated peer that can ever arrive** (a roam to an attacker-supplied or
dead address is §7.3's own threat model). The connection neither sends nor
dies. That is the **immortal half-open session** ruling 182's beacon proof
exists to forbid (`SPEC.md:2691–2696`), reached without a single suppression
of `Liveness`.

### 4.2.2 What the gate is and is not responsible for — **read this before ruling**

**The announce-gate did not create this.** Remove `keepalive_can_leave()` from
`sync_liveness_timer` and the same state announces `last_send +
KEEPALIVE_TIMEOUT`, already in the past; `handle_timeout` →
`transmit_keepalive_if_owed` → `transmit_keepalive` refuses at `mod.rs:3019`
without moving `last_send` → re-arm in the past → F1's **spin**. Nothing arms
the death clock on that path either. So:

| | pre-F1 | post-F1 (shipped) |
|---|---|---|
| the connection dies? | **no** | **no** |
| the driver | spins at 100 % of a core | parks on `Timeout(None)` |

The gate is a strict improvement and is **not** the defect. The defect is
that §7.4's `armed` bit and §7.5's passive debt are set by the same event, so
the passive keepalive is the *only* thing that can arm the death clock in its
own window — and §7.3 can veto it.

**Consequence for the round-41 clause, which is the point of this
investigation:** ruling 249's soundness sentence must **not** be copied
verbatim into a keepalive clause. *"the connection's `Timeout` falls to the
next armed timer — `Liveness` at the latest"* is **true for `Pto`** (a `Pto`
is armed only when the sent map is non-empty, which means an ack-eliciting
send happened, which means `armed == true`) and **false for the passive
keepalive**. Drafting it in would ship a false rationale — precisely working
rule 11's failure mode, on the maintainer's own text, one round after the
last two.

### 4.2.3 Status of this finding

**Reported, not resolved (working rule 3), and not measured.** I am read-only
and did not build the construction; every link is cited above but the whole
has not been driven. Per the project's own "measure before ruling" note the
next step is a paused-clock / sans-io fixture at §4.2.1's six steps, asserting
`Timeout(None)` **with `liveness().is_armed() == false`** — the assertion
`the_passive_form` says nothing in `tests_livelock.rs` currently makes.
If the construction fails, the failing link is almost certainly step 4/5 (the
ACK's reachable size against the room), and the gate is then sound as ruling
249 claims — but `the_passive_form`'s own argument still needs the "~35 bytes"
step replaced, because that step is false as written.

### 4.2.4 The beacon is the safe half

For `PersistentKeepalive` the picture is different and **sound**: the beacon
is unconditional and does not consult `R` (`mod.rs:2880–2884`), so a
connection with a beacon configured always has *some* marking send pending;
and `an_admissible_beacon_still_fires_and_advances` (`tests_livelock.rs:681`)
plus `the_beacon_returns_when_the_hold_lifts` (`:954`) pin fire and re-arm.
The beacon's suppression is bounded by the receive that funds it. The hole in
§4.2.1 is specific to the **passive** keepalive — the one with no held-state
test (`tests_livelock.rs:727–732`).

---

# 5. Rule-4 sweep — prose a gate clause would have to answer

Greps run: `announce|announced|announcement`; `keepalive|Keepalive|KEEPALIVE`
(whole file, partitioned by section); `budget|amplification|§7.3` inside
§7.4–§7.5; `always announced|always armed|is always|unconditional`;
`90|3 ×` inside §7.3.

| # | `SPEC.md:line` | text | verdict |
|---|---|---|---|
| 1 | `:6093–6100` (§16.5) | *"`Pto` and `Contested` … **both wait on the same budget predicate**"* | **CONFLICTING (scope).** Four timers wait on a budget predicate in the shipped code. Introduced by ruling 249. Must be reworded by any keepalive clause. |
| 2 | `:6076–6078` (§16.5) | the eight-name timer table | consistent — no arming rule stated for `Keepalive`/`PersistentKeepalive` anywhere in §16.5. That absence is where a cross-ref goes. |
| 3 | `:2681–2685` (§7.5) | passive rule: *"a side that has received since its last **marking** send, and has not made a **marking** send for `KEEPALIVE_TIMEOUT`, **sends a keepalive**"* | consistent-but-silent. A firing rule with no announcement half; under rule 8 a reader takes it as unconditional. **This is the normative home.** |
| 4 | `:2705–2708` (§7.5) | beacon: *"it fires when no marking send has occurred for the configured interval, and re-arms from every marking send"* | same — silent, not contradicting. |
| 5 | `:2723–2728` (§7.5) | *"The beacon is *unconditional*: it fires on its own timer and **asks nothing of `R`**"* | consistent. "Unconditional" is explicitly scoped to `R`, not to §7.3. A clause should keep that scoping visible so the two words do not read as opposites. |
| 6 | `:2619–2627` (§7.4) | *"whose beacon is unconditional by design (§7.5) and therefore does fire into the unvalidated anchor … the session emits at most 588 B for the replayed 196 B and **then goes quiet until the address validates**"* | **consistent, and load-bearing.** This is the keepalive's exact twin of §13.4's *"emits nothing, and the session dies at `DEAD_TIMEOUT`"* — **the intended outcome is already ratified; only the announcement rule is missing.** Ruling 249 leaned on precisely this shape. |
| 7 | `:2205`, `:2419–2421`, `:2430` (§7.3) | keepalives are cwnd-exempt but budget-bound; *"the budget binds all output and cannot be waived"*; **rank 7** | consistent. Already covers `transmit_keepalive`'s **guard**. Nothing here about the timer. |
| 8 | `:2297–2298`, `:2445–2446` (§7.3) | the 90 B floor | consistent — and it is the load-bearing premise of the whole soundness argument (§4.1). Any clause should cite it. |
| 9 | `:4852` (§14.5) | exemptions: *"Non-ack-eliciting control packets — pure ACKs, CLOSE, keepalives"* | consistent. |
| 10 | `:3959` (§9) | *"no error, no timeout, and keepalives still flowing in both directions, so liveness never fires"* | consistent — describes a **validated** address with the dance running; unrelated to the gate. |
| 11 | `:7271–7278` (Appendix B) | the **PTO-disarm** obligation ruling 249 added | consistent; a keepalive clause wants a sibling obligation, largely already discharged by `tests_livelock.rs`. |
| 12 | `:5969–5977` (§16.4) | *"an event fired at the mark would announce a countdown that is not running"* | consistent — the principle a keepalive clause cites, unchanged. |
| 13 | `:4738–4751` (§13.6) | the exhaustive roam table | **incomplete** — no keepalive row. See finding C-3. |

## 5.1 Finding C-4 — a **conflict**, reported not resolved (working rule 3)

`SPEC.md:2290–2300` (§7.3), the roam seam's no-deadlock proof:

> If nothing at the new address ever answers, nothing is validated and the
> session dies by liveness inside 25 s — **unconditionally**, since any
> ack-eliciting output we aim at the address arms the death clock by itself,
> even where nothing marking is sent (§7.4); `PATH_CHALLENGE` is itself
> ack-eliciting (§8.3), so the very packet that asks the question arms the
> clock on the answer. **There is no deadlock in either direction: the budget
> always admits *something*** (the anchoring or roaming packet funds 3× its
> own size, and 3× the *smallest* packet that can arm the budget — §7.5's
> 30-byte keepalive, 90 B — still admits a packet carrying the challenge…).

**This proof reasons about §7.3's budget and only §7.3's budget.** It is
silent on §14.5's congestion gate, which can hold the `PATH_CHALLENGE`
independently — the challenge is **not** cwnd-exempt (§14.5's list;
`mod.rs:2387–2394` reads that list as closed under rule 8), and §13.6 states
in ratified text that right after a roam *"`bytes_in_flight` may exceed the
fresh initial window; the admission gate then blocks new sends"*
(`SPEC.md:4715–4719`).

When that happens the ratified rank order works **against** the proof:
rank 5 (pure ACKs) is **above** rank 6 (PTO probes) and the pure ACK is
cwnd-exempt, non-marking and non-ack-eliciting, and is **sized to the whole
remaining room** (`mod.rs:2137–2149` + `ack.rs:163–171`). So the 90 B the
proof relies on can be spent by the one output class that arms nothing —
ahead of both escapes the proof and §13.6 name:

* the `PATH_CHALLENGE` (rank 4) — held by cwnd;
* §13.6's stated fallback, *"the path stays probeable by the PTO exemption
  within that budget"* (`SPEC.md:4719–4720`) — but post-ruling-249 the `Pto`
  **announcement** is itself budget-gated (`mod.rs:1563`), so a room spent to
  0 by the ACK suppresses the probe that was the escape.

**So §7.3's "no deadlock in either direction" and §13.6's "a bounded stall of
at most one loss-detection/PTO cycle" both appear to have an unstated scope,
and ruling 249's `Pto` gate narrowed the second one.** This is the same
finding as §4.2 seen from the spec side rather than the code side. **Reported,
not resolved.** It is arguably a larger item than round-41 item 1 itself, and
it needs measurement before it needs a ruling.

---

# 6. Candidate clauses

§7.5's bullet list runs: keepalive `:2679–2699`, persistent keepalive
`:2700–2840`, Liveness `:2841–…`. **The insertion point for a normative
bullet is after `SPEC.md:2840`, before `:2841`** — after both keepalives (the
gate covers both) and immediately before the death clock (which is what the
soundness sentence must be careful about).

## Option A — one normative bullet in §7.5 (insert after `SPEC.md:2840`)

```markdown
- **Neither keepalive's deadline is announced while a keepalive cannot
  leave.** **[RATIFIED 2026/08/1X — ruling 2XX]** `Keepalive` and
  `PersistentKeepalive` (§16.5) are armed only while §7.3's budget admits
  the 30-byte empty plaintext **and** no contested mark is pending — rank 2
  outranks rank 7 (§7.3), so a keepalive may not spend budget the probe is
  waiting for. Both deadlines are functions of `last_send`, and a held
  keepalive is held **without moving `last_send`**, so arming from it
  regardless puts the deadline at an instant already passed: the shell's
  `sleep_until` returns immediately, the same timer re-fires, and the one
  `!Send` driver every connection shares spins (§16.3) — invisible on the
  wire and silent in release.

  **The gate is on the announcement, not the state.** `last_send`, the
  passive rule's debt and §7.4's arming bit are untouched while a keepalive
  is held; only the deadline is withheld, and the connection's `Timeout`
  falls to whatever else is armed. **Arming nothing is right and arming
  later is wrong**: both holds lift only on a *received* packet — the budget
  grows only at an authenticated, window-fresh receive (§7.2, §7.3, ruling
  169), and a pending mark clears on an ACK covering its floor — so no
  future instant is predictable and there is no correct deadline to arm.
  Every such receive recomputes both deadlines, so they are restored on the
  one event that can lift the hold; no dedicated re-arm machinery exists.
  This is §16.4's `Contested` principle — a deadline is never announced for
  output that cannot leave — applied to §7.5's own two timers, and it is
  what §7.4 already describes from the other side: *"the session emits at
  most 588 B for the replayed 196 B and then goes quiet until the address
  validates."*
```

**Tradeoff:** smallest possible edit, lands the rule where the arming rules
already live, and mirrors ruling 249's placement (normative beside the timer,
not in §16.5). Leaves §16.5's false enumeration and §13.6's table untouched.

## Option A′ — Option A plus the honest backstop paragraph

Append to Option A's bullet:

```markdown
  **`Liveness` is deliberately not gated**, and the reason it is not is
  §7.4's, not this rule's: suppressing the death clock alongside the
  keepalives would trade a spinning connection for an immortal one. But
  `Liveness` is **not** a backstop for a held keepalive the way it is for a
  held `Pto` (§13.3), and the difference is stated here rather than left to
  be discovered. A `Pto` is armed only with a non-empty sent map, which
  implies an arming send, so a `Pto`-suppressed connection always has its
  death clock running. §7.4 disarms the death clock at every authenticated,
  window-fresh receive — the **same event** that sets the passive rule's
  *"has received since its last marking send"* — so in the passive
  keepalive's own window the death clock is armed by nothing but the
  keepalive the budget has just vetoed. The floor that closes it is §7.3's:
  every receive that sets the debt funds at least 3 × 30 = 90 B, which is
  more than the 30 B the keepalive asks and more than the 31 B whose refusal
  keeps a mark pending, so the hold and the debt cannot be set by the same
  event.
```

**Tradeoff:** this is the only draft that states the soundness argument
instead of asserting the wrong one — but its **last sentence is exactly the
claim §4.2 puts in doubt**, because §7.3's rank 5 (pure ACKs, cwnd-exempt,
sized to the whole room) can spend that 90 B before anything arms. Ratifying
A′ as written would ship a rationale whose mechanism has not been checked —
working rule 11's failure mode. **A′ must not be ratified until §4.2.1 is
measured.**

## Option B — §16.5 only (reword `SPEC.md:6096–6099`)

Replace:

> they are not independent in one respect: **both wait on the same budget
> predicate**, `Contested` at the probe's transmission (§16.4) and `Pto` at
> the announcement;

with:

> they are not independent in one respect: **four of the eight timers wait on
> §7.3's budget** — `Contested` at the probe's **transmission** (§16.4),
> `Pto` at its **announcement** (§13.3), and `Keepalive` and
> `PersistentKeepalive` at theirs (§7.5), the last two additionally waiting
> on the absence of a **pending** contested mark. The sizes asked differ —
> 39 B for the probe's challenge datagram (§13.4), 30 B for §3.4's empty
> plaintext — but the predicate is one predicate;

**Tradeoff:** mandatory regardless of which option is chosen, because the
current sentence is false (finding C-2). But §16.5 is a *restatement* layer —
ruling 249 put its normative sentence in §13.3 and only restated it here — so
this alone leaves the keepalives with no normative arming rule anywhere.

## Option C — A (or A′) **plus** B **plus** the §13.6 row **plus** Appendix B

The full shape ruling 249 used. Additional edits:

**§13.6 table, new row after `SPEC.md:4744`** (the `Pto` row):

```markdown
| `Keepalive` / `PersistentKeepalive` | **announcements suppressed** — the roam zeroes the budget four rows up, so §7.5's announce-gate withholds both deadlines until the first qualifying receive; `last_send`, the passive debt and §7.4's arming bit carry across untouched | §7.5, §7.3 |
```

**Appendix B, sibling of the PTO-disarm obligation at `SPEC.md:7271–7278`:**

```markdown
- **Keepalive-disarm** (§7.5, ruling 2XX): at a budget with no room for the
  30-byte empty plaintext, and — separately — at a **pending** contested
  mark with room for the keepalive but not the probe, **neither** keepalive
  deadline is announced. The two holds must be separated (a build consulting
  only one passes the other's test), the announcement must return at the
  receive that lifts the hold, and the admissible path must still fire and
  re-arm — a build that suppresses unconditionally satisfies every
  non-retrospection assertion and has deleted §7.5.
```

This obligation is **already discharged** by `tests_livelock.rs` — tests 1, 3,
7, 8 and 10 in §2's table map onto its four clauses one-for-one. That is
unusual and worth saying in the ruling: the test file predates the clause and
is more precise than ruling 249's description of it.

## Recommendation

**Option C, with A (not A′) as the §7.5 bullet, and a separate ruling — or an
explicit `[OPEN]` — for the backstop question.**

Reasons, in order:

1. **B is not optional.** `SPEC.md:6096–6099` is false today and ruling 249
   made it false. Whatever else round 41 does, that sentence is wrong.
2. **The normative home is §7.5**, by ruling 249's own precedent: it put the
   arming conjunct in §13.3 where `Pto`'s arming rule lives, not in §16.5.
   §7.5 is where both keepalive rules live and where §16.5 has no arming rule
   at all.
3. **§13.6's table is declared exhaustive** by ruling 173 and the roam is the
   principal way into the suppressed state. Ruling 249 discharged this duty
   for `Pto`; the same duty attaches here.
4. **A, not A′,** because A′'s closing sentence is the one claim §4.2 shows is
   not established. Shipping A leaves the soundness argument unstated, which
   is honest; shipping A′ states an argument whose mechanism has not been
   measured, which is the defect working rule 11 exists for. The backstop
   question deserves its own ruling with its own measurement.

## Amendment-table row (insert in the table at `SPEC.md:17–27`, after ruling 254's row)

```markdown
> | 2XX | §7.5, §16.5, §13.6, Appendix B | neither keepalive deadline is announced while §7.3's budget or a pending contested mark holds the packet — slice 7b's shipped gate, stated; §16.5's "both wait on the same budget predicate" was two timers and is four |
```

---

# 7. VERDICT

**Base:** `94dab20`, clean tree, verified before the first read.

**1. Is the shipped gate sound?** **Partly — and the interesting half is not
the half ruling 249 asserted.**

* The **gate itself** is sound and is a strict improvement: it withholds an
  announcement, never state (`mod.rs:2934–2946`), and re-arms on the one
  event that can lift the hold — `sync_liveness_timer()` is called on the
  receive path at `mod.rs:637`, one line after the credit at `:633`, which is
  a *shorter* re-arm path than `Pto`'s. `the_beacon_returns_when_the_hold_lifts`
  (`tests_livelock.rs:954`) pins it. **No livelock; the driver parks.**
* The **backstop** does not transfer. Ruling 249's *"the `Timeout` falls to
  the next armed timer — `Liveness` at the latest"* is true for `Pto` and
  **false for the passive keepalive**: `Liveness::deadline()` returns `None`
  unless `armed` (`session.rs:309–312`), and `armed` is cleared by the very
  receive that sets the passive debt (`session.rs:336–344`). The passive
  keepalive is owed exactly where the death clock is off.
* The blind author flagged this (`tests_livelock.rs:873–928`) and argued it
  was unconstructible on a 90 B floor. **The floor step is right; the step
  after it is wrong.** A pure ACK does not cost "~35 bytes" — it is sized to
  the whole remaining room (`mod.rs:2137–2149`, `ack.rs:163–171`,
  `MAX_ACK_RANGES` 64) and neither marks nor arms (`SPEC.md:2641–2643`). The
  author said it was guessing at exactly that clamp and reported rather than
  resolved; it was right to.
* **§4.2.1 gives a six-step construction** reaching `Timeout(None)` with **no
  timer armed at all** — an immortal, silent connection. It is **traced, not
  measured**, and is reported for measurement, not ratification.
* **The gate is not the cause.** Pre-F1 the same state spins instead of
  parking, and dies in neither. The defect is that §7.4's `armed` bit and
  §7.5's passive debt are set by one event, so the passive keepalive is the
  only thing that can arm the death clock in its own window — and §7.3 can
  veto it.

**2. Where does the clause belong?** **§7.5 normative, §16.5 restated, §13.6
a row, Appendix B an obligation** — the exact shape ruling 249 used for `Pto`,
with §7.5 standing in for §13.3 because that is where the keepalive arming
rules live. §16.4 needs nothing.

**3. Preferred draft:** **Option C built on Option A** (§6). Not A′ — its
closing sentence asserts the very soundness argument §4.2 puts in doubt, and
ratifying it would ship a rationale naming an unchecked mechanism (working
rule 11).

## Findings, numbered

| # | finding | kind |
|---|---|---|
| **C-1** | Ruling 249's scope guard describes the keepalive gate as *"on the budget"*; it is `!contested.is_pending() && admits(30)` — two conjuncts, and `tests_livelock.rs`'s whole one-byte calibration exists to separate them. A clause drafted from 249's sentence would be too narrow. | imprecision in `rulings.md`; would propagate |
| **C-2** | `SPEC.md:6096–6099` (§16.5, added by ruling 249): *"both wait on the same budget predicate"* — four timers do. Working rule 8, introduced one round after the keepalive gate shipped. | **spec conflict, must fix** |
| **C-3** | §13.6's exhaustive roam table (`SPEC.md:4738–4751`) has no keepalive row, though a roam zeroes the budget and suppresses both announcements. | spec omission |
| **C-4** | §7.3's *"There is no deadlock in either direction"* (`SPEC.md:2290–2300`) reasons only about the budget. §14.5's cwnd can hold the `PATH_CHALLENGE` (not exempt) while rank-5 pure ACKs (exempt, sized to the room, arming nothing) spend the 90 B — and post-249 the `Pto` escape §13.6 names (`:4719–4720`) is itself budget-gated. | **conflict, reported not resolved** |
| **C-5** | The passive keepalive has **no held-state test** anywhere (`tests_livelock.rs:727–732`, `:873–928`); the state is reachable only if C-4 is, and the fixture cannot express it — working rule 13's shape. | coverage gap |

## What round 41 should actually do

1. Ratify **Option C/A** — the documentary gap ruling 249 deferred. Cheap,
   uncontroversial, and C-2 forces part of it anyway.
2. Open **C-4 as its own item** and **measure §4.2.1 before ruling on it**.
   It is a liveness question, not a documentation question, and it is larger
   than item 1.
3. Correct **C-1** in the round-41 ruling's text so the two-conjunct shape is
   on the record (working rule 4(b): a ruling citing another inherits the duty
   to address its reasoning).

---

## Appendix — constants verified at base (`src/constants.rs`)

`DATA_HEADER_LEN` 14 (`:90`) · `AEAD_TAG_LEN` 16 (`:119`) ·
`MAX_ACK_RANGES` 64 (`:356`) · `AMPLIFICATION_FACTOR` 3 (`:466`).
So `keepalive_can_leave` asks about **30 B** and `probe_can_leave` about
**39 B**, as both doc comments claim.

Repo untouched: `git status --porcelain` empty and `HEAD` still `94dab20` at
the end of the investigation.

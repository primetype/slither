# CONTRACT-7 — the binding API contract for slice 7 (mobility and contest)

**Status: BINDING.** Round 30 (rulings 168–184) has ruled every item this
file previously marked ⚠-unruled, including the planner's two open
questions — **and it changed both of their stated defaults**, so a reader
working from a remembered default will build the wrong thing. Every
slice-7 agent compiles against this file and none may change it. If you
believe something here is wrong, **say so in your report and implement it
as written anyway** (working rule 5).

Base commit for the worktree cut: **named in your brief**. This file and
the `testutil`/`testfix` extension of PLAN-7 §3 are both in it (working
rule 14).

House style carried from `CONTRACT-5a.md` and `CONTRACT-6.md`: **every
return value is stated for every state, including the ones that look
obvious.** A slice-4a test author guessed two *semantic* things wrong —
including whether `read` distinguishes `Ok(Some(0))` from `Ok(None)` —
because a contract was not in the cut commit. Nothing here is left to
inference.

---

## §0. The rulings that govern slice 7 — these override anything below

**Read this table before any spec text quoted later in this file.** Where
a ruling and a quoted §-section disagree, **the ruling wins.** This table
demonstrably saved slice 5a, where the extracted spec text predated a
round of rulings.

| # | Decision |
|---|---|
| **35** | §6.4's PENDING branch **runs §6.7's comparison**. Peer's static smaller ⇒ cancel the pending, install as responder. Ours smaller ⇒ `AcceptError::Stale`, pending left in place, candidate's timestamp **recorded**. The tie-break is a two-sided agreement; one side cannot opt out. |
| **36** | The **contested-connection probe**. A §6.4 refusal against a `None` basis marks the connection contested, sends an ack-eliciting PING, and demands an ACK within `KEEPALIVE_TIMEOUT`. Declined at the root: a 12-byte timestamp in msg2 — **wire-affecting, do not re-propose.** |
| **39** | The keepalive dance is **automatic for any connection that has carried traffic**. A connection with **no authenticated receive since install** dies at install + `DEAD_TIMEOUT` in silence. Declined: making all keepalive opt-in. |
| **40** | `PERSISTENT_KEEPALIVE`'s bound is a **ceiling, not a floor** — reject at or above `DEAD_TIMEOUT`. Default 25 s → **10 s**. The beacon **stays in the marking set**. Ruling 38 is reversed in part; its derivation is retained as the proof. |
| **42** | The beacon also needs a **floor**: reject below **1 s**. Admissible range **`[1 s, DEAD_TIMEOUT)`** — 1 s inclusive, 25 s exclusive. |
| **41** | The probe's predicate is a **counter high-water mark, not a packet identity**. Clear on **any ACK covering any counter ≥ `probe_floor`**. Collapse concurrent marks into a **single** contested state, one floor, one deadline. Declined: retransmittable PING; clearing on any post-mark ACK without a floor. |
| **43** | ⛔ **Its rate bound is SUPERSEDED by ruling 175; its accounting clause is LIVE.** ~~one per `KEEPALIVE_TIMEOUT` per connection~~ — see 175 below. **Still binding:** the probe **is counted in the sent map** and in `bytes_in_flight`, so §17.5's cwnd bound stays true. **Also still binding, and reconciled by ruling 188:** 43 denied the characterisation *"bounded by the application's own accept rate"* because **the attacker supplies the Intros**, and that denial stands against §7.5's original *"bounded by … nothing an attacker controls"*. Ruling 175 restores the accept-rate phrase in a **narrower** sense — the application's `accept()` calls are the **rate ceiling** (no attacker can cause one), while **which** of those calls becomes a probe is **attacker-chosen** (the attacker supplies the parked `Intro`s each one refuses against). Both halves are true; state both. |
| **44** | `set_persistent_keepalive` returns **`Result<(), ConfigError>`** with `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}`. **Never a panic** (reachable across bubble-ffi to iOS, where unwinding is UB), **never a silent clamp** (reports success while giving a beacon that does not do what was asked). A rejected call leaves the interval **unchanged**. |
| **45 → 46** | Ruling 45's `ConnEvent::Contested { under_probe: bool }` **is repaired by 46**: the core enum reaches no application. The shell gets `closed()` and `notified()`. **FAB-6**: emit at **probe transmission**, not at marking, and split the variants — `under_probe: false` read as the *pending* state while meaning *cleared*, and the three real states do not map onto one bool. |
| **90** | `core::Endpoint::connect()` is **split** into `mint_pending` (0 DH, synchronous) and `start_attempt` (2 DH, on the driver). `connect` is **deleted, not kept as a wrapper**. `start_attempt` is a **no-op for an unknown `ConnectionId`**. |
| **91 + amendment** | §6.5, §6.6 **and §6.4's PENDING branch** moved from slice 7 **into slice 4**. What remains of §6.4 for slice 7 is the **LIVE branch**. ⚠ See §1.2. |
| **63** | The persistent-keepalive **ceiling gets no named constant** — compare against `DEAD_TIMEOUT` directly. *"A named ceiling would be a second place `DEAD_TIMEOUT` is written down, and therefore a place it can drift."* |
| **117** | The shell's waker maps key by **`StreamRef`**, never `StreamId` — a tie-break can invert opener parity. Slice 7 adds a waker map; it follows this. |
| **130** | §13 and §14's **machinery** is slice 5's. Slice 7 owns only the **roam seam** through them (§13.6, §14.6). |
| **137** | `SentPacket` carries a **`u32` path generation**, landed in slice 5 at 0, **made live by slice 7**. One recovery marker cannot serve §13.6's four fences. ⚠ See §3.4. |
| **139(a)** | `pto_count` increments **at the `Pto` timer's firing**, before the probe is built — *"the one that stays right in slice 7 where §7.3's budget can prevent a probe leaving."* A budget-blocked probe still counts. |
| **152 / 128** | The post-death drain covers `recv_message` and `recv_datagram` as well as `read`/`accept_*`. **Parking is never permitted on a dead connection.** A `Notification` *"is a fact about the connection, complete in itself, and it survives the event it describes."* |
| **155** | One datagram per packet, packed **before** the stream fill; the `0x30` extends-to-end form is **mandatory**, not an optimisation. ⚠ Its interaction with "PING last" is §7.2. |

### Round 30 (2026/08/16) — rulings 168–184, **the governing set for this slice**

Taken after an adversarial spec audit run blind to the planner. **Eleven
of twelve findings upheld.** These override every §-section quoted below.

| # | Decision |
|---|---|
| **168** | **The amplification budget disarms on a return-routability proof. This reverses a recorded declination.** §7.3's *"never lifted"* made every responder-side connection permanently capped — *"an endpoint that accepts connections could never serve one."* At each address change, record `validation_floor` = **the counter the next seal will use**. The address becomes **validated** and the budget disarms when an authenticated, window-fresh packet **from that address** carries an ACK covering **any counter ≥ `validation_floor`**. One `u64`, one `bool`, no new frame. **The 3× ratio itself is still never lifted** — never raised, never configurable; what ends is the *unvalidated state*. ⚠ Flagged by the maintainer as the ruling most wanted attacked in the post-slice protocol review. |
| **169** | The budget is funded by **authenticated *and window-fresh*** bytes. §7.3's exclusion list said only "unauthenticated or undecryptable", which on the literal text let a **replayed** packet replenish a security counter. |
| **170** | The budget is **per session**, not per address. Both constant-table sites are corrected to *"per unvalidated address, per session"*. The residual — N sessions to one address multiply the reflector by N — is stated, not inferred. |
| **171** | **(a) A pending contested probe takes priority over all other output to an unvalidated address** — ahead of ACKs, keepalives, PTO probes, retransmissions and new Data. §7.5's congestion-gate argument transfers verbatim, and the budget cannot be waived, so priority is the only lever. **(b)** §7.3 states the full priority order. |
| **172** | **The code's 2/2 fence split is ratified.** `recovery_start` fences the **congestion event** and **`app_limited` growth**; `path_gen` fences the **RTT sample** and the **persistent-congestion walk**. The defect was documentation, not behaviour. **Also: ruling 137's fences are not open debt — slice 5 closed the design; slice 7 owes wiring.** |
| **173** | §13.6 grows an explicit list of **every** per-connection reset on the roam seam, because its title claims a scope its body does not cover. Without it an implementer building `on_roam()` from §13.6 lets the budget carry the old address's credit to the new one — *"the reflector §7.3 exists to prevent, reconstructed out of a missing line."* |
| **174** | **`timers.rs`'s order is authoritative.** §16.5 gains the two missing relations: *teardown collection precedes loss/PTO evaluation*, and *loss/PTO/`AckDelay` precede keepalive evaluation*. The second does **not** follow from §16.5's governing principle — `AckDelay` before `Keepalive` is emission-before-emission — so it is stated, not derived. |
| **175** | The probe-rate bound is restated honestly: *"at most one probe per mark, at most one mark per uncontested refusal, and marks cannot overlap; the refusal rate is the application's own `accept()` rate."* Ruling 43's *"one per `KEEPALIVE_TIMEOUT`"* was wrong — a live peer ACKs in ~1 RTT and clears the mark, so the next refusal is a full second mark. **No cooldown is added** (it would leave a genuine second doubt unprobed). **Every re-mark records a fresh floor.** |
| **176** | **The pending mark's two unstated exits.** Clearing a pending mark **cancels the pending probe and emits nothing** — no `Contested`, no `ContestCleared`. *"`ContestCleared` is emitted only where `Contested` was."* A **roam while pending leaves the mark intact with its floor unchanged** (the counter space is never reset). |
| **177** | **A contested mark requires an *admitted* candidate** — one proving the same static with a verifying tail tag. §6.9's "any refusal" scoping is corrected. Otherwise an attacker who can only park mac1-valid rubbish provokes marks with no key material. **Re-home-walk exhaustion joins §6.4's `Stale` list** and marks nothing. |
| **178** | **PENDING means membership in the pending tables, not a datagram in flight.** Ruling 90's split left this open and ruling 91 recorded it unresolved. §6.4, §6.5 and §17.4 are reworded from *"an in-flight outbound initiation exists"* to *"a pending exists for the proven static"*. |
| **179** | §6.4's admission rule carries the closing/draining carve-out **explicitly**, where the mark is taken — *"a rule enforced only in the section that describes the state rather than the section that enters it is a rule that gets missed."* |
| **180** | **`FlakyWire::rebind(new_addr)`.** `addr` becomes a `Cell`; `Network` moves the `EndpointState` between keys **carrying the existing `Rc<Notify>`** (the driver's recv loop is parked on that exact `Rc`; a fresh one hangs it); rebinding onto a registered address panics. **In-flight datagrams are abandoned, not carried** — that is what a real rebind does, and the carry-across alternative would let S18 pass *for the wrong reason*. `src/testutil/mod.rs` lands **committed before dispatch**. |

### The planner's two open items are now ruled — **both defaults changed**

| # | Decision |
|---|---|
| **181 + 184** | **Q5 is settled by arithmetic, not precedence.** A maximum-size datagram in `0x30` form is 1 + 1169 = **exactly `MAX_PLAINTEXT` (1170)**, so it shares its packet with **nothing** under any rule — that *is* ruling 155's documented bias, already ratified. The collision arises only for a **sub-maximum** extends-to-end frame, and there **PING is packed immediately before it**. An extends-to-end frame's final position is *structural* (nothing can follow a frame with no length prefix); PING's "last" is merely *ordinal*. **The planner's default is superseded and its test instruction inverted: you MAY assert that a probe rides a data packet, provided the datagram aboard is sub-maximum.** The `0x31` escape is **withdrawn for datagrams** (ruling 155 makes `0x30` mandatory) and remains open to STREAM frames. |
| **182** | **Q7: yes, and for a stated reason.** The passive keepalive reads `S` = last **marking** send. §7.5's prose *"has not sent"* is corrected. This is the **first time in this project the formal rule held the intent rather than the prose** — because the beacon's soundness proof rests on *"every send that can establish `S > R` is a marking send, so the death clock is armed there"*, which the prose reading collapses into an immortal half-open session. **Which sends are marking is §7.4's answer — quote it, do not re-derive it.** |
| **183** | **A paused-clock test-authoring instruction, and it will bite you.** `Congestion::reset(now)` sets `recovery_start = now`, and `in_recovery` tests `sent_time <= start`. A **post-roam** packet sent in the *same virtual instant* as the roam therefore **is** fenced. On tokio's paused clock that is the norm, not an edge case. **A test asserting "a post-roam loss cuts cwnd" must advance the clock after the roam, or it asserts the opposite of what its name says.** The `≤` stands — it is §14.3's ordinary rule and the roam case errs conservatively (a fenced packet suppresses a cwnd *cut*, never inflates a window). |
| **184** | Amends 181; see above. Also records the process finding: three parties reasoned about Q5 from §8.5's words and got three answers, when two constants in a ruling all three had read settled it. **When a spec conflict is about capacity, do the arithmetic before taking a position.** |

**Everything else the planner flagged, Round 30 ruled.** Q1 → 176 (and
the three-state shape below stands); Q2 → 168/169/170; Q3 → 172;
Q5 → 181/184; Q6 → 174; Q7 → 182; Q9 → 168/173; Q10 → 176; Q12 → 180;
Q14 → 178; Q16 → 178; Q13, Q15, Q17, Q18 stand as recorded in
`QUESTIONS-7.md`.

**Nothing in this contract is now marked ⚠-unruled.** If you find
yourself reasoning from a default, you are reading a stale copy.

---

## §1. Scope of slice 7

### 1.1 In

§7.3 roaming + the anti-amplification budget · §7.5 keepalive, persistent
keepalive, the contested probe · §13.6 / §14.6's roam seam · §5.4 and
§6.4's **LIVE** branch (replacement) · §6.8 restart · §16.2's
`notified()` + `Notification`.

**Stories:** S3 (a/b/c), S4, S5, S11, S18, S19, S20, S27 (full).

### 1.2 Out — and one boundary to verify first

**Out:** §6.5's routing, §6.6's internal tie-break, and — per ruling 91's
amendment — **§6.4's PENDING branch**. All three landed in slice 4.

⚠ **Verify before writing (Q14).** Read
`src/core/endpoint/staged.rs` and `routing.rs` at your base commit. If
the PENDING branch is there, S4 is an **acceptance-verification** story:
write its tests, run them, and a pass with no implementation change is
the correct outcome.

**Also out:** §13 and §14's machinery (ruling 130), the frame layer, the
packet layer, `src/packet/`, the intro queue, the timestamp guard.

### 1.3 The wire does not move

Slice 7 adds **no frame type, no header field, no constant**. PING is
already `0x01`. The keepalive is §3.4's **empty plaintext** and carries no
frames at all. Rulings 36, 41, 43, 45, 46 and 137 each say *"no wire
change"* / *"wire-free"* in terms.

`src/constants.rs:668–685` asserts the frame table has exactly **14**
named types. **A red golden-wire or size/constant test in slice 7 is a
bug in slice 7, not an expectation to update.**

---

## §2. Constants — all frozen, all already in `src/constants.rs`

| Constant | Value | Line |
|---|---|---|
| `KEEPALIVE_TIMEOUT` | 10 s | `constants.rs:416` |
| `DEAD_TIMEOUT` | 25 s | `:419` |
| `PERSISTENT_KEEPALIVE_DEFAULT` | 10 s | `:422` |
| `PERSISTENT_KEEPALIVE_MIN` | 1 s, **inclusive** | `:431` |
| `AMPLIFICATION_FACTOR` | 3 | `:438` |
| `INTRO_TTL` | 15 s | `:447` |
| `REPLAY_WINDOW` | 2048 bits | `:159` |
| `FRAME_PING` | `0x01` | `:169` |
| `MAX_DATAGRAM` | 1200 | `:138` |
| `INIT_PACKET_LEN` / `RESP_PACKET_LEN` | 196 / 107 | `:128` / `:131` |

**The persistent-keepalive ceiling has no constant** and must not acquire
one (ruling 63). Compare against `DEAD_TIMEOUT` directly.

**Add no constant this slice.** `tests/spec_constants.rs` pins the table.

---

## §3. Core API additions — `core::Connection<C: Handshake>`

All new items are `pub(crate)`, matching the existing surface. `now:
Instant` is an argument on every mutating call; the core never reads a
clock.

### 3.1 Roaming

```rust
impl<C: Handshake> Connection<C> {
    /// The address this connection's datagrams go to — §5.6's anchor,
    /// moved by §7.3's roaming.
    ///
    /// Returns `None` before a session is installed. **Total after
    /// install**, and the shell's `remote_address()` mirrors it.
    pub(crate) fn remote_address(&self) -> Option<SocketAddr>;

    /// §14.6's path-generation stamp (ruling 137). `0` at construction;
    /// `+= 1` at each **committed** roam, never at a rejected one.
    #[cfg(test)]
    pub(crate) fn path_generation(&self) -> u32;

    /// §7.3's budget, for tests. `None` when the address is **validated**
    /// (no budget armed); `Some((sent, received))` in **datagram bytes**
    /// when it is unvalidated.
    #[cfg(test)]
    pub(crate) fn amplification_budget(&self) -> Option<(u64, u64)>;
}
```

`handle_datagram`'s signature **does not change**:

```rust
pub(crate) fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
```

`src` is currently discarded at `mod.rs:276` (`let _ = src;`). That line
goes.

**The roam predicate, exhaustively.** A roam is committed **iff all four
hold**:

1. The packet is a **Data** packet (handshake packets never reach this
   core, and §7.3 L1898 forbids them roaming a live session anyway).
2. `session.open(...)` returned `Some` — the **AEAD tag verified**.
3. The replay window **marked** it — `check_and_mark` accepted it as
   fresh. A duplicate or a counter more than 2048 behind is **not** fresh.
4. `src != current anchor`.

and **iff** the connection's `Lifecycle` is `Live`. §15.2 and
`mod.rs:1289` are explicit: **a closing or draining connection does not
roam.**

**On commit, in this order** (§16.4's generation order is normative):

| # | Effect |
|---|---|
| 1 | `from = anchor`; `anchor = src` |
| 2 | `path_generation += 1` |
| 3 | `Recovery::on_roam(now)` — sent map **kept**, `min_rtt` re-seeded |
| 4 | `NewReno::reset(now)` — cwnd = `INITIAL_WINDOW`, ssthresh = `u64::MAX`, `recovery_start = Some(now)` |
| 5 | The budget arms: both counters reset, then the **triggering packet's datagram length** credits the received counter. **RULED (168/169):** correct as written — the roam trigger is authenticated *and* window-fresh by §7.3, which is exactly what ruling 169 requires of anything that funds the budget. The same step records `validation_floor`. |
| 6 | `tracing::debug!(target: "slither::roam", …)` with `from` and `to` |
| 7 | `ConnEvent::AddressMoved { from, to }` is queued |

**What a roam does NOT do** — stated because a list is read as exhaustive
(working rule 8):

- It does **not** clear the sent map (§13.6 L3804).
- It does **not** reset `bytes_in_flight`.
- It does **not** reset `pto_count`, `loss_time`, or `last_ack_eliciting`
  — *"loss detection and PTO continue undisturbed"* (§13.6 L3806).
- It does **not** clear `smoothed_rtt` or `rttvar` — the estimator is
  *"suspect-but-kept"*. Only `min_rtt` is re-seeded, and the **PTO floor
  may rise** as a result.
- It does **not** reset the replay window, the flow-control state, any
  stream, the counter, or the session keys.
- It does **not** re-handshake, and it does **not** touch the endpoint.
  §17.4: *"the endpoint tracks no per-connection address."*

### 3.2 The anti-amplification budget (§7.3, rulings 168–171, 173)

**Per session** (ruling 170), never per address. Four fields on the
connection:

```rust
/// §7.3's budget. `validated == true` ⇒ the other three are meaningless
/// and no check runs.
validated: bool,
validation_floor: u64,   // ruling 168
budget_sent: u64,        // datagram bytes sent to the current address
budget_recv: u64,        // authenticated AND window-fresh datagram bytes from it
```

**Armed** — `validated = false`, `budget_sent = 0`, `budget_recv = 0`,
`validation_floor = session.next_counter()` — at exactly two events, and
no others:

1. A committed roam.
2. An **accepted initiation's msg1 anchor** — the connection was created
   by `accept()`. The msg1 qualifies as authenticated, *"its handshake
   tail tags having verified at admission"*, and credits `budget_recv`.

**Not armed** for a `connect()`-supplied address: a dialled connection
starts **validated**. **This is the single most load-bearing fact for the
contested-probe tests** — see §8.3.

**Disarming — ruling 168, and this is new.** The address becomes
**validated**, and the budget stops binding, when an **authenticated,
window-fresh packet from that address carries an ACK covering any counter
≥ `validation_floor`**. That ACK can only have been produced by a peer
that received something we sent to that address *after* the change — a
return-routability proof, wire-free.

Note the construction is **identical to ruling 41's `probe_floor`** and
uses the same `session.next_counter()`. They are two independent floors
recorded at different moments; do not conflate them, and do not share one
field.

**Without this, an accepting endpoint could never serve a connection**:
a peer downloading a file replies with ACKs only, ~40 bytes per ~2400
sent, funding ~120 bytes of budget against 2400 bytes of demand.

**The check.** Before any datagram leaves for an **unvalidated** address:

```text
budget_sent + datagram.len()  >  AMPLIFICATION_FACTOR * budget_recv
    ⇒ the datagram is HELD — not dropped, not truncated, not an error.
```

**Binds all output**, explicitly including the §14.5 and §13.4
congestion-window exemptions — PTO probes, the contested probe, pure
ACKs, CLOSE, and keepalives. *"Those exemptions are scoped to cwnd, never
to this budget."* In code, the budget check sits **outside** the `!probe`
guard at `mod.rs:1517–1527`.

**Priority within a scarce budget — ruling 171, binding.** When the
budget admits less than is owed:

1. **A pending contested probe** — ahead of everything.
2. Owed ACKs.
3. PTO probes.
4. Retransmissions.
5. Keepalives.
6. New application Data.
7. CLOSE.

A probe the budget could delay past its own deadline *"would silently
convert congestion into a liveness verdict"* — and the budget cannot be
waived, so priority is the only lever.

**Replenishment — ruling 169.** Only packets that are **authenticated
*and* window-fresh** credit `budget_recv`. A **replayed** packet credits
nothing, even though it authenticates: the literal §7.3 text excluded only
"unauthenticated or undecryptable" and thereby let a keyless on-path
attacker inflate our send budget. Credit at exactly the point §7.2's
window marks the packet — the same instant liveness is refreshed
(`session.rs:559–561`).

**Units: datagram bytes, both directions** — the full UDP payload,
`Transmit::data.len()` on the send side, matching ruling 136's convention
for `SentPacket.size`.

**The residual, stated rather than inferred** (ruling 170): N sessions to
one address multiply the reflector by N.

**No deadlock, by design.** A budget-blocked path dies by liveness inside
25 s, *"unconditionally so: any ack-eliciting output we aim at the address
arms the death clock by itself."*

### 3.3 Keepalive and persistent keepalive (§7.5)

```rust
impl<C: Handshake> Connection<C> {
    /// §7.5's beacon. `None` disables it.
    ///
    /// Returns `Err(ConfigError::KeepaliveTooShort)` for an interval
    /// **strictly below** `PERSISTENT_KEEPALIVE_MIN` (1 s), and
    /// `Err(ConfigError::KeepaliveTooLong)` for one **at or above**
    /// `DEAD_TIMEOUT` (25 s). On `Err` the current interval is
    /// **unchanged** — no clamp, no panic (ruling 44).
    ///
    /// `None` is accepted at all times, including on a dead connection.
    pub(crate) fn set_persistent_keepalive(
        &mut self,
        now: Instant,
        interval: Option<Duration>,
    ) -> Result<(), ConfigError>;

    /// The configured beacon interval, or `None` if disabled.
    pub(crate) fn persistent_keepalive(&self) -> Option<Duration>;
}
```

**The admissible band, every boundary stated:**

| Argument | Result |
|---|---|
| `None` | `Ok(())` — beacon disabled |
| `Duration::ZERO` | `Err(KeepaliveTooShort)` |
| `1 ms`, `500 ms`, `999 ms` | `Err(KeepaliveTooShort)` |
| **`1 s` exactly** | **`Ok(())`** — the floor is **inclusive** |
| `1 s + 1 ms` … `24.999 s` | `Ok(())` |
| **`25 s` exactly (`DEAD_TIMEOUT`)** | **`Err(KeepaliveTooLong)`** — the ceiling is **exclusive** |
| `30 s`, `Duration::MAX` | `Err(KeepaliveTooLong)` |

App. B L6178–6181 names both regressions: *"a test that pins the old
floor at `DEAD_TIMEOUT` is the regression this obligation exists to
catch, and a test asserting that 1 s is rejected is the mirror
regression."*

**The passive keepalive rule.** With
`S = liveness.last_send()` and `R = liveness.last_authenticated_recv()`:

> Arm `TimerKind::Keepalive` at `S + KEEPALIVE_TIMEOUT` **iff `R > S`**.
> On firing, send the **empty plaintext** via `seal` (a **marking** send).

⚠ **Q7: `S` counts marking sends only.** A `seal_quiet` send — a PTO
probe, a credit frame, a retransmission, the contested PING — does **not**
advance `S` and does **not** suppress the keepalive.

**The beacon rule.**

> Arm `TimerKind::PersistentKeepalive` at `S + interval`. It **re-arms
> from every marking send** and is **not** reset by receives. It fires
> **unconditionally** — it does not consult `R`.
> On firing, send the **empty plaintext** via `seal` (marking).

**Both keepalives are marking.** Arming enables death, never defers it. A
connection whose entire output is beacons still dies at
`R + DEAD_TIMEOUT`.

**Keepalives never enter the sent map**, are never ack-eliciting, never
occupy the congestion window (§8.7, §14.5), and are **never
retransmitted**. They are admitted to the peer's replay window and appear
opportunistically in ACK ranges.

**A connection with no authenticated receive since install** has
`S == R` at install with the deadline armed there, so `R > S` is false
from the start. It emits **nothing** and dies at install + 25 s.

### 3.4 The roam fences (§13.6, §14.6, ruling 137)

`SentPacket.path_gen` is stamped at seal time from
`Connection.path_generation`. The `debug_assert_eq!(packet.path_gen, 0,
…)` at `recovery.rs:198` **is deleted** — it is ruling 137's tripwire and
its removal marks the field going live.

**Ruling 172 ratifies the code's 2/2 split.** This is binding and it is
**not** what a plain reading of §14.6's closing clause gives:

| Fence | Mechanism | Predicate |
|---|---|---|
| congestion event | **`recovery_start`** | `pkt.time_sent <= recovery_start` |
| `app_limited` growth | **`recovery_start`** | `pkt.time_sent <= recovery_start` |
| RTT sample | **`path_gen`** | `pkt.path_gen != self.path_generation` |
| persistent-congestion walk | **`path_gen`** | `pkt.path_gen != self.path_generation` |

`recovery_start` is set to the roam instant by `NewReno::reset(now)` and
is **never cleared** (ruling 139(b)) — it only ever moves forward — so
every pre-roam packet keeps `time_sent <= recovery_start` **permanently**
and cannot escape the fence by resolving late. That is why the split is
behaviourally identical to a uniform stamp for pre-roam packets, and why
ruling 172 calls the defect *documentation, not behaviour*.

`path_gen` cannot be replaced by `recovery_start` for the other two
because `recovery_start` is also set by **every ordinary congestion
event**, so reusing it would suppress RTT sampling after every normal loss
episode — silently, and permanently on a lossy path (ruling 137).

**What still happens to a pre-roam packet:** it **resolves for loss and
retransmission normally**. Its frames re-queue by §8.7's class. It leaves
`bytes_in_flight` when acked or declared lost. The fences suppress
*feedback*, never *recovery*.

**⚠ Paused-clock note for test authors, and it is not a defect in the
ruling.** The two `recovery_start` fences use `<=`. On a paused clock a
packet sent **after** the roam but in the **same virtual instant** has
`time_sent == recovery_start` and is therefore fenced as though it were
pre-roam: its loss fires no congestion event and its ACK grows no window.

**A test asserting "a post-roam loss cuts cwnd" MUST advance the clock
between the roam and the send.** Not doing so produces a red against a
conformant implementation, and the integrator will read it as a bug in the
implementer's work. The `path_gen` fences have no such sensitivity.

### 3.5 The contested state — see §5

Specified in §5, with the notification, because the two are pinned to one
instant and splitting them here would invite the collapse S11 forbids.

---

## §4. Core API additions — `core::Endpoint<I: Identity>`

Slice 7 changes **one** call site and adds **no** new endpoint verb.

### 4.1 §6.4's LIVE branch — `src/core/endpoint/staged.rs`

Today, `staged.rs:617–621`:

```rust
Some((StaticState::Live, _)) => {
    self.discard_chain(now, id);
    return Err(AcceptError::Stale);
}
```

Slice 7 replaces it with §6.4's §16.1 guard. `accept()`'s signature is
**unchanged**:

```rust
pub(crate) fn accept(&mut self, now: Instant, id: IntroId)
    -> Result<(ConnectionId, Connection<I::Suite>), AcceptError>;
```

**The decision table, exhaustive.** `basis` is the live connection's
`replacement_basis` (§17.4), `t_cand` the admitted candidate's timestamp:

| Case | `accept()` returns | Old connection | Guard record |
|---|---|---|---|
| admitted candidate, `basis = Some(t)`, `t_cand > t` | `Ok((new_id, new_conn))` | **`ConnectionLost::Replaced`**, fired **at this call** | **kept** |
| admitted candidate, `basis = Some(t)`, `t_cand <= t` | `Err(AcceptError::Stale)` | untouched, **not** contested | **reverted** |
| admitted candidate, `basis = None` | `Err(AcceptError::Stale)` | untouched, **marked contested** | **reverted** |
| admitted candidate, `basis = None`, connection **closing or draining** | `Err(AcceptError::Stale)` | untouched, **NOT marked** — ruling 179 | **reverted** |
| **no initiation parked** for that static | `Err(AcceptError::Stale)` | untouched, **NOT marked** | n/a |
| **re-home walk exhausted** — candidates parked, all failed | `Err(AcceptError::Stale)` | untouched, **NOT marked** — ruling 177 | **reverted** |
| static is PENDING and we are the tie-break winner | `Err(AcceptError::Stale)` | n/a | **kept** — the one exception |

**Ruling 177 — the mark requires an *admitted* candidate**, one proving
the same static with a verifying tail tag. §6.9's looser "any refusal"
scoping is corrected. Without this, an attacker who can only park
mac1-valid rubbish provokes contested marks **with no key material at
all**, while §6.9 prices those rows at *"0 DH … one bounded queue slot"*
and never at a probe.

**Ruling 179 — the closing/draining carve-out is enforced here**, at the
admission, not only in §7.5. A closing connection remains in §17.4's map
for its linger, so §6.4's rule as written would take a mark that §7.5 then
has to undo.

**Ruling 178 — "PENDING" means a pending exists in the pending tables**,
not that a datagram has left. Ruling 90's `mint_pending` /
`start_attempt` split made a minted-but-not-yet-sent pending possible;
it still counts as PENDING. If it did not, a peer's initiation arriving
in that window would take §5.4's NONE row, we would install as responder,
and `start_attempt` would *then* fire — two key sets, both msg2s dropped,
mutually dark for `DEAD_TIMEOUT`, *"exactly the divergence ruling 35 was
made to prevent."*

**There is no `AcceptError::AlreadyConnected`** — §6.4 L1362–1365 says the
variant is unreachable and deleted. `ConnectError::AlreadyConnected`
remains, for `connect()`.

**The replacement installs a fresh connection.** §5.4 L820–823: *"a fresh
connection with fresh transport state on both sides … **no stream,
flow-control, recovery, or congestion state ever crosses a
handshake**."* ⚠ Q15: ruling 117's rationale reads otherwise; §5.4
governs. The new connection has fresh streams, fresh credit, fresh
recovery, fresh counters, a fresh replay window, and its **own**
`replacement_basis` (`Some(t_cand)`, since we are its responder).

**The `Replaced` teardown fires exactly at the replacing `accept()`,
never earlier.** A withheld or replayed initiation left unaccepted costs
one parked `Intro` and nothing else.

**The old connection's death is a no-linger teardown**: `ConnEvent::
Closed(ConnectionLost::Replaced)` followed **within the same drain** by
`ToEndpoint::Retired { our_index }` (§16.4 L5027–5029).

### 4.2 `connect()` — S3a, unchanged behaviour, restated

`Endpoint::mint_pending` already returns
`Err(ConnectError::AlreadyConnected)` when `statics.get(&key).is_some()`
— **live or pending alike** (`endpoint/mod.rs:412–414`). S3a needs no
implementation change. **The first connection is untouched.**

The application-facing consequence, which S3a's ⚠ CHECK requires be
documented: *"an application that wants 'reconnect now' must `close()`
first, then `connect()`."*

---

## §5. Events and notifications

### 5.1 The contested state (⚠ Q1)

```rust
/// §7.5's contested mark. **Three states, not two** — §16.4's ruling-46
/// rationale names them: *"the three real states (marked-pending,
/// probing, cleared) do not map onto one bool at all."*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Contested {
    /// Not contested.
    No,
    /// Marked. The floor is recorded; §7.3's budget has not yet admitted
    /// the PING. **No deadline is armed and nothing has been emitted.**
    Pending { floor: u64 },
    /// The PING went out at `armed_at`; the verdict is due at `deadline`
    /// (= `armed_at + KEEPALIVE_TIMEOUT`).
    Armed { floor: u64, armed_at: Instant, deadline: Instant },
}
```

**Marking** — when §6.4 refuses against a `None` basis:

| Current state | Effect |
|---|---|
| `No`, connection `Live` | `floor = session.next_counter()`; state ⇒ `Pending { floor }`; trace `slither::policy` "mark" **with the floor**. **No PING, no timer, no event.** |
| `No`, connection **closing or draining** | **Total no-op.** §7.5 L2446–2448: *"already leaving, and the parked `Intro` will meet no live static."* |
| `Pending { .. }` | **Total no-op.** Floor unchanged, no PING, no timer, no event. |
| `Armed { .. }` | **Total no-op.** Floor unchanged, **deadline NOT re-armed**, no second PING, no event. This is a **security property**, not an optimisation (§7.5 L2331–2337). |

**Transmission** — at the first instant §7.3's budget admits the PING (on
a validated address, that is the **same instant** as the mark):

1. Send an ack-eliciting **PING**, exempt from the cwnd gate (`probe:
   true`), **counted in the sent map and in `bytes_in_flight`**.
2. State ⇒ `Armed { floor, armed_at: now, deadline: now + KEEPALIVE_TIMEOUT }`;
   arm `TimerKind::Contested` at `deadline`.
3. Queue `ConnEvent::Contested`.
4. Trace `slither::policy` "transmission".

**These four are one atomic step.** §15.4 L4093 pins them: *"which is
also when the deadline arms and when `Contested` is emitted."*

**Clearing** — on **any** ACK covering **any** counter `>= floor`. The
effect depends on the state, and **ruling 176 is the reason**:

| State when the covering ACK lands | Effect |
|---|---|
| `Armed { .. }` | State ⇒ `No`; disarm `TimerKind::Contested`; queue **`ConnEvent::ContestCleared`**; trace "verdict: cleared". |
| **`Pending { .. }`** | State ⇒ `No`; **cancel the pending probe — the PING is never sent**; **emit NOTHING**; trace "verdict: cleared (pending)". |
| `No` | Nothing. |

**The pending exit is not exotic.** The floor is *"the counter the next
seal will use"*, so **any** post-mark seal — a keepalive, a
retransmission, a pure ACK, application Data — lands at or above it. On
the literal pre-ruling text `ContestCleared` would fire **with no
preceding `Contested`**, which is exactly the unmatched-notification
mis-read ruling 46 deleted `under_probe: bool` to prevent; and the
unconditional send rule would emit a **stray probe** arming a
`KEEPALIVE_TIMEOUT` verdict for a mark that no longer exists.

**`ContestCleared` is emitted only where `Contested` was.** The
*"mark-pending gap emits nothing"* principle governs the gap's **exit**
as well as the gap.

**A roam while `Pending` leaves the mark intact, with its floor
unchanged** (ruling 176). The counter space is never reset (§7.7), so the
floor stays meaningful across the roam. It is listed among §13.6's roam
resets **as a thing that is not reset**.

**No cooldown, and every re-mark records a fresh floor** (ruling 175). A
live peer ACKs in ~1 RTT and clears the mark, so the **next** refusal is a
full second mark with a **new** floor. The honest bound is *"at most one
probe per mark, at most one mark per uncontested refusal, and marks cannot
overlap; the refusal rate is the application's own `accept()` rate."*
Ruling 43's *"one per `KEEPALIVE_TIMEOUT`"* is **superseded** — do not
write a test asserting it.

In every clearing case the refusal **stands**; the basis rule is
untouched.

**Verdict** — `TimerKind::Contested` fires:

1. `ConnEvent::Closed(ConnectionLost::TimedOut)` — **the same variant as
   liveness, no new one** (§15.4 L4105–4120).
2. `ToEndpoint::Retired { our_index }` in the same drain.
3. **Nothing is transmitted.**
4. Trace `slither::policy` "verdict: timed out".

**No third notification.** A mark that is never answered emits
`Contested` (at transmission) and then nothing — the death arrives on
`closed()`.

**Emitted at most once per mark**, each. Ruling 41's collapse bounds marks
to one per connection at a time, so both inherit that bound.

### 5.2 `ConnEvent` — three new variants

```rust
pub(crate) enum ConnEvent {
    // ... the eleven that exist at b649575, unchanged ...

    /// §7.3's roam committed. `from` is the previous anchor, `to` the new.
    AddressMoved { from: SocketAddr, to: SocketAddr },
    /// §7.5's contested probe **went out** (rulings 45/46). Never at the mark.
    Contested,
    /// An ACK covered the probe floor; the mark cleared.
    ContestCleared,
}
```

Field names are **exactly** `from` and `to`, both `SocketAddr`, matching
§16.4 L4907 and §16.2 L4230. `Contested` and `ContestCleared` are **unit
variants** — no fields, ever (ruling 46 removed `under_probe: bool`).

Insert them in §16.4's declared order: after `DatagramReadable`, before
`Closed`. Note the tree also carries `SendCreditAvailable` (ruling 150,
slice 6) which §16.4's list predates; leave it where it is.

### 5.3 `Notification` — the shell's public enum

```rust
/// The application-facing notification set — deliberately **not**
/// `core::ConnEvent`. Non-exhaustive: a later wire line may add a kind
/// without a breaking change.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    /// §7.3's roam committed.
    AddressMoved { from: SocketAddr, to: SocketAddr },
    /// §7.5's contested-connection probe went out.
    Contested,
    /// That mark cleared — an ACK covered the probe floor.
    ContestCleared,
}
```

**Exactly three variants.** It does **not** mirror `ConnEvent`:
`StreamReadable`, `MessageReadable` and the rest are already served by the
blocking verbs, *"and duplicating them would give two ways to learn the
same thing."*

### 5.4 Retention — one slot per kind

```rust
pub(crate) struct NotificationSlots {
    /// Merged: **oldest unclaimed `from`, newest `to`**.
    address_moved: Option<(SocketAddr, SocketAddr)>,
    contested: Option<u64>,        // generation
    contest_cleared: Option<u64>,  // generation
    address_moved_gen: u64,
    next_gen: u64,
}
```

The shape above is illustrative; **the behaviour below is binding.**

| Rule | Statement |
|---|---|
| **O(1)** | Retention is **one slot per kind**, never a queue. There is no bound to configure because there is nothing to bound (§17.5). |
| **`AddressMoved` merge** | An unclaimed `AddressMoved` superseded by a further roam keeps the **oldest unclaimed `from`** and the **newest `to`**, *"so the pair always describes the net move since the application last looked."* |
| **`Contested` / `ContestCleared`** | Distinct kinds. **At most one *mark* exists at any instant** (ruling 41's collapse) — but that is a statement about marks, **not** about slots, and an earlier draft of this table read it as the latter. Ruling 175: a live peer clears in ~1 RTT and the next refusal is a full second mark, so `Contested` → `ContestCleared` → `Contested` is ordinary traffic and **a slot may be rewritten any number of times**. |
| **Ordering** | Pending notifications of different kinds are handed over in **generation order** (§16.4's ordering rule). |
| **Second write to an occupied slot** | **RULED (185): the latest write takes the slot and the new generation.** With no drain, `Contested`(g1) → `ContestCleared`(g2) → `Contested`(g3) hands over as *cleared, then contested* = "contested now". Keeping the old generation would hand over *contested, then cleared* = **"cleared now", the exact inverse of the truth** — reporting a contested connection as healthy, which is the one error S11 exists to prevent. |
| **Survives death** | A notification generated before the death is **still claimable after it**. `notified()` returns `Err(ConnectionLost)` **only once every slot is drained.** |

### 5.5 `slither::policy` — the three traced moments

§18.2 L5730 is operator-visible contract. The probe traces **three**
moments and slice 7 must emit all three:

| Moment | Carries |
|---|---|
| the **mark** | the probe floor |
| the **transmission** | (the instant; the deadline follows) |
| the **verdict** | cleared, or `TimedOut` |

*"an operator wants the gap and an application does not."* The mark trace
is the **only** observable of the pending state below the shell.

`slither::roam` carries endpoint moves. It is a ratified target that
nothing currently emits on.

---

## §6. Shell API additions

```rust
impl<S: Handshake> Connection<S> {
    // ... everything at b649575, unchanged ...

    /// Claim **one** notification, in the same pull model as
    /// `accept_bi`, `recv_message` and `recv_datagram`.
    ///
    /// The connection **retains** what has not been claimed and this
    /// hands over exactly one, so a notification is never dropped on the
    /// floor between an application's two visits.
    ///
    /// Resolves `Err(ConnectionLost)` once the connection has ended
    /// **and** its unclaimed notifications have been drained — a
    /// notification generated before the death is not lost to the death.
    ///
    /// **Cancel-safe**: a dropped future has claimed nothing, and the
    /// next call yields the same notification.
    pub async fn notified(&self) -> Result<Notification, ConnectionLost>;

    /// §7.5's beacon. See `ConfigError` for the admissible band.
    ///
    /// Synchronous: it is a shared-cell write like the accessors, not a
    /// command-channel round trip.
    pub fn set_persistent_keepalive(
        &self,
        interval: Option<Duration>,
    ) -> Result<(), ConfigError>;

    /// **[ADDED post-dispatch — ruling 189]** The configured beacon
    /// interval, or `None` when the beacon is off.
    ///
    /// Added because ruling 44 makes *"a rejected call leaves the interval
    /// **unchanged**"* an acceptance criterion that **nothing in this
    /// surface could observe** — so the obligation was testable only by
    /// inferring the interval from beacon cadence across a
    /// 3 × `DEAD_TIMEOUT` window. A setter whose effect cannot be read
    /// back is the defect; this is the fix. Same shared-cell read as the
    /// accessors below.
    ///
    /// **Integrator's note:** this postdates the worktree cut, so the
    /// implementer's brief does not contain it. It is additive and lands
    /// at integration.
    pub fn persistent_keepalive(&self) -> Option<Duration>;

    /// The address this connection's datagrams go to.
    ///
    /// **Total** — never panics, never `Option`. §7.3's roaming is what
    /// makes it change; before install it is the address `connect()` was
    /// given.
    pub fn remote_address(&self) -> SocketAddr;   // exists; now non-constant
}
```

**`notified()` is written once as `poll_notified`**, per `CLAUDE.md`'s
handle-seam rule; the `async fn` is `poll_fn` over it.

**The waker slot.** `ConnCell` gains **one** `Wakers` field,
`notification_wakers`, alongside the five that exist. It is **not** swept
by `take_all_stream_wakers` — the precedent is `closed_wakers`, which is
deliberately excluded (`shared.rs:367–370`): a notification survives the
death, so its waiters must be woken by the drain, not by the teardown
sweep.

**Ruling 58 applies.** `notified()` claims **at most one item, and only
inside `poll`**. An adapter that claims ahead of its consumer rebuilds
the unbounded queue §10.6 forbids while looking like a convenience.

**`ConfigError`** is already declared at `src/error.rs:293,296` and pinned
by `tests/spec_errors.rs`:

```rust
pub enum ConfigError {
    KeepaliveTooShort,   // strictly below PERSISTENT_KEEPALIVE_MIN (1 s)
    KeepaliveTooLong,    // at or above DEAD_TIMEOUT (25 s)
}
```

It sits **outside §18.1's protocol taxonomy**, which stays closed: no
peer, no packet, no connection state, nothing observable on the wire.

**Nothing else on the shell surface changes.** In particular `closed()`,
`acked()`, `read`'s `Ok(Some(n))` / `Ok(None)` (FIN, not reset) and every
accessor are exactly as at `b649575`.

---

## §7. Wire — nothing moves

### 7.1 No new frame type

PING is `0x01` and already fully wired: `Frame::Ping` (`frame.rs:70`),
`type_code` (`:96`), `encoded_len` = 1 (`:117`), `encode` (`:156`), the
parse arm (`:712`), `is_ack_eliciting → true` (`:791`),
`retransmission → Never` (`:825`), `Packing::ping()` (`:948`).

The keepalive is §3.4's **empty plaintext** — a 16-byte tag-only
ciphertext, a **30-byte datagram** — and bypasses the frame layer at
`mod.rs:321–326` and `frame.rs:686–690`.

### 7.2 The packing collision — **RULED (181 + 184)**

§8.5 gives two frames "final position": *"then PING last"* and *"at most
one extends-to-end frame (¬LEN STREAM, or `0x30` DATAGRAM) per packet, in
final position."*

**The two claims are not the same kind of claim.** An extends-to-end frame
carries **no length prefix**, so it runs to the end of the packet by
definition: its final position is **structural**, and anything following it
would be parsed as part of it. PING's "last" is **ordinal** — a placement
preference among length-prefixed frames, and a 1-byte frame's position
carries no semantics.

**And for the case everyone worried about, the collision does not exist.**
Ruling 155 fixes `MAX_DATAGRAM_PAYLOAD` = 1169 and `MAX_PLAINTEXT` = 1170.
A maximum-size datagram in `0x30` form is `1 + 1169 = 1170` bytes —
**exactly the whole plaintext**. It shares its packet with *nothing*: no
PING, no ACK, no control frame, no stream fill. That is not a new
constraint; it **is** ruling 155's documented bias.

So:

| case | rule |
|---|---|
| maximum-size `0x30` datagram | alone in its packet. No packing rule applies because nothing else fits. |
| **sub-maximum** extends-to-end frame | **PING is packed immediately before it.** |
| STREAM frames | the LEN form remains available; ruling 155 does not constrain it. |
| datagrams | the `0x31` escape is **withdrawn** — ruling 155 makes `0x30` mandatory, and the sizes where `0x31` fits are exactly the sizes where nothing was blocking. |

**This inverts the earlier instruction.** A test **may** assert that a
probe rides a data packet, provided the datagram aboard is sub-maximum.
The previous draft told you not to write one; that default is superseded.

### 7.3 PING's retransmission class is unchanged

`Never` (§8.7). *"A lost PING is superseded by the next probe."* Making it
retransmittable was **declined** (ruling 41) — do not re-propose.

---

## §8. Observable ordering guarantees

### 8.1 Within one drain

**Output ordering preserves generation order** (§16.4 L5053–5055,
normative): *"a transmit and the event it caused come out in that
order."* So on a roam: `Transmit`s already queued, then whatever the roam
produces, then `Event(AddressMoved)` — never the event before the effect.

On a probe transmission: `Transmit` (the PING) **then**
`Event(Contested)`.

### 8.2 Equal-deadline timer order (ruling 174 — **ratified**)

`timers.rs`'s declaration order is **authoritative**, and §16.5 gains the
two relations it was missing. Note the second — *loss/PTO/`AckDelay`
precede keepalive evaluation* — **does not follow from §16.5's stated
governing principle**, because `AckDelay` before `Keepalive` is
emission-before-emission. It is stated, not derived. Do not try to
re-derive this order from §16.5's prose; read it here:

```
Liveness  >  CloseLinger  >  Contested        (teardown collection)
          >  Loss  >  Pto  >  AckDelay        (recovery evaluation)
          >  Keepalive                        (emission)
          >  PersistentKeepalive              (the beacon, last)
```

Read left-to-right as "fires first". Additionally:

- **`Loss` suppresses `Pto`** at the same instant — exactly one of the two
  fires per evaluation, and the suppressed `Pto` **stays armed**.
- **`Liveness` beating `Contested`** at the same instant is *"the harmless
  ordering"* — both produce `ConnectionLost::TimedOut`.
- **`PersistentKeepalive` is evaluated last**: any marking send the
  instant produced re-arms it, so it does not fire redundantly.
- A session already collected for teardown **owes no keepalive**.

### 8.3 The mark / transmission / verdict separation

**Binding, and the single most testable property in the slice:**

| Moment | `notified()` yields | `slither::policy` traces |
|---|---|---|
| the mark, budget not yet admitting | **nothing** | the mark, with its floor |
| the transmission | `Notification::Contested`, at that exact instant | the transmission |
| an ACK ≥ floor | `Notification::ContestCleared` | verdict: cleared |
| the deadline with no such ACK | **nothing** — `closed()` yields `TimedOut` | verdict: timed out |

**The pending gap is reachable only on a connection that has roamed**
(§3.2). A test for "the notification fires at transmission" that does not
roam first proves nothing — the two instants coincide.

### 8.4 What a roam is observable as, on each side

| | The peer moved | **We** moved |
|---|---|---|
| our `ConnEvent::AddressMoved` | **fires** | **does not fire** |
| our `remote_address()` | **changes** | **unchanged** |
| our `notified()` | yields `AddressMoved` | yields **nothing** |
| the peer's side | nothing | fires `AddressMoved` |

S19's acceptance is entirely **peer-side**. A test that asserts a local
`AddressMoved` after our own rebind is asserting something that can never
happen.

---

## §9. Fixture extensions — available at your base commit

Landed by the integrator **before** the worktrees were cut. Both test
authors and the implementer may use them; **none may edit them.**

**Ruling 180.** The verb is on **`FlakyWire`**, not `Network`:

```rust
// src/testutil/mod.rs
impl FlakyWire {
    /// Move this wire to `new_addr`, as an interface change or a NAT
    /// rebind does. `Network` moves the `EndpointState` between keys,
    /// **carrying the existing `Rc<Notify>`** — the driver's recv loop is
    /// parked on that exact `Rc`, and a fresh one hangs it.
    ///
    /// **In-flight datagrams are abandoned**, not carried: the new
    /// address gets a fresh empty inbox, and the old key is vacated so
    /// anything sent there after the rebind drops as it already would.
    ///
    /// Panics if `new_addr` is already registered, as `Network::endpoint`
    /// already does.
    pub fn rebind(&self, new_addr: SocketAddr);

    /// This wire's **current** local address. Changes under `rebind`.
    pub fn local_addr(&self) -> SocketAddr;
}
impl SharedWire {
    /// Every clone observes the move — the address lives in one `Cell`
    /// behind the shared `Rc`, so the driver's own clone sends from the
    /// new address on its next send, with no re-plumbing.
    pub fn rebind(&self, to: SocketAddr);
}
impl Peer {
    pub fn rebind(&self, to: SocketAddr);
    pub fn addr(&self) -> SocketAddr;   // was a pub field; now current
}

// src/core/connection/testfix.rs
impl Solo {
    /// `deliver`, with an explicit source address, for roam tests.
    pub(crate) fn deliver_from(&mut self, now: Instant, src: SocketAddr, frames: &[u8]) -> Drained;
}
impl Pair {
    pub(crate) fn flush_a_to_b_from(&mut self, now: Instant, src: SocketAddr) -> Drained;
    pub(crate) fn flush_b_to_a_from(&mut self, now: Instant, src: SocketAddr) -> Drained;
}
```

**Why abandoning in-flight datagrams is load-bearing, not a shortcut**
(working rule 9): under a carry-across rebind a peer could move, **stay
silent, and still receive** — so S18's central claim, *"the mover must
send, and the keepalive is what does it"*, would pass **for the wrong
reason**. Abandonment is what makes the positive obligation bite.

It is pinned by
`testutil::tests::a_datagram_in_flight_to_the_old_address_is_lost_on_rebind`,
and **verified by mutation**: built against the carry-across alternative
the ruling declined, that test fails on its own assertion — not on an
internal guard, and not on a later one.

### 9.0 Two details settled in the landing, beyond the sketch above

1. **`Peer::addr` is a method, and the field is gone** — not deprecated,
   removed. A field cannot follow a `rebind`, so after a move it would
   hold the address the peer *used to* have, and every mobility test is
   exactly a test about which address is current. It would not fail
   loudly; it would assert the pre-move address and **pass**. 99 call
   sites moved to `addr()`.
   *A consequence worth knowing before you hit it:* Rust 2021 closures
   capture disjoint **fields**, so `|| ... peer.addr ...` used to leave
   `peer.endpoint` movable. A **method** borrows the whole struct, so a
   closure that reads the address now conflicts with a later
   `drop(peer.endpoint)`. Read the address into a local before the
   closure — `src/shell/mod.rs` has the worked case.
2. **Rebinding to the address you already hold is a no-op, not a panic** —
   and specifically it does **not** drop the inbox, which the naive
   remove-then-insert would. Pinned by
   `rebinding_to_the_same_address_keeps_the_inbox`.

A rebind also clears any `partition`/`block_path` involving the old
address: those are properties of an **address**, not of a wire, and a
fresh address is by definition neither partitioned nor blocked. Re-apply
them to the new address if your test needs them held across the move.

`Network::inject(from, to, bytes)` already exists and is the **forgery**
fixture — it bypasses `Tap`, `Network::sends()` and every `FlakyPolicy`
knob, and it is **one-way** (the reply has nowhere to land unless `to` is
registered). Use it for M2's negatives, not for a round trip.

**`testfix::parse_frames` needs no new arm.** Slice 7 adds no frame type.
The keepalive's empty plaintext yields `vec![]`.

### 9.1 Standing fixture limits — unchanged, and they bound your tests

- `FlakyPolicy::lossy(rate)` is **invisible to every public counter** —
  the tap sits above the loss draw. **Only `block_path`/`drop_at` prove a
  drop.**
- `Tap` yields **sealed datagrams**, so no integration test can count
  frames. It **does** carry `Spied { src, dst, bytes }`, so it can prove
  an address change.
- `LocalSet::run_until` re-polls its body on any local-task wake, so a
  test awaiting from the `local()` body **cannot detect a lost wakeup**.
- `settle()` yields **without advancing the paused clock**.
- `FlakyWire` models a *network*, not a *socket*: it cannot express "this
  send fails" or "this driver panics".

---

## §10. Error variants — none are added

| Type | Slice-7 change |
|---|---|
| `ConfigError` | **none.** Both variants exist at `error.rs:293,296` and are pinned by `spec_errors.rs`. Slice 7 **constructs** them for the first time. |
| `ConnectionLost` | **none.** `Replaced` exists and is constructed for the first time. The contested verdict reuses `TimedOut` — **no new variant** (§15.4 L4105–4120). |
| `ConnectError` | **none.** `AlreadyConnected` already covers S3a and the tie-break-loser's cancelled pending. |
| `AcceptError` | **none.** `Stale` covers every refusal. **There is no `AcceptError::AlreadyConnected`** — deleted as unreachable (§6.4 L1362–1365). |
| `IntroError`, `AuthError`, `WriteError`, `ReadError`, `MessageError`, `DatagramError` | **none.** |

**§18.1's taxonomy stays closed.** A diff in `src/error.rs` that adds a
variant is a ruling request, not an implementation detail.

---

## §11. Ownership of shared files (working rule 15)

| Path | Owner | Note |
|---|---|---|
| `src/testutil/mod.rs` | **integrator**, pre-slice | in the cut commit; nobody edits it during the slice |
| `src/core/connection/testfix.rs` | **integrator**, pre-slice | same |
| `Cargo.toml` | **integrator**, post-slice | the `[[test]]` stanzas for `story_keepalive` and `story_mobility`, each `required-features = ["test-util"]`. **Cargo does not warn about a `[[test]]` whose file is missing — it refuses to parse the manifest**, so an implementer landing live stanzas commits a tree on which *no gate can run at all*. If you are the implementer and you feel you need a stanza: **you do not.** Land nothing, and say so in your report. |
| `tests/story_keepalive.rs` | **T-K only** | the implementer creates nothing here |
| `tests/story_mobility.rs` | **T-M only** | the implementer creates nothing here |
| `src/core/connection/tests_roam.rs` | **implementer only** | its own unit tests; a distinct filename from any story file |
| `tests/spec_constants.rs`, `tests/spec_errors.rs` | **integrator** | expect no change; a needed change is a ruling request |

**No path appears twice.** In slice 2a an implementer's placeholder stub
overwrote 68 independently written tests because two briefs named one
path. If you need a module to compile against, declare `#[cfg(test)] mod
…;` and **create nothing**.

---

## §12. Worked sequences

### 12.1 S18 — the peer moves, mid-stream

```
t=0     A and B established. A dialled, so A's address for B is validated;
        B accepted, so B's address for A was anchored from msg1.
t=0..   A writes a stream; B reads it. Both sides now have R > S at times,
        so the keepalive dance is running.
t=T     B's network changes. B's wire rebinds to addr_c.
t=T+ε   B's next marking send — its passive keepalive, per S18's
        "the mover must send and the keepalive is what does it" —
        arrives at A from addr_c.
        A: open() authenticates → check_and_mark marks it fresh →
           src != anchor → ROAM COMMITTED.
           anchor := addr_c; path_generation += 1;
           Recovery::on_roam; NewReno::reset; budget arms.
           Transmit(s), then Event(AddressMoved { from: b_addr, to: addr_c }).
        A's remote_address() now returns addr_c.
        A's notified() yields Notification::AddressMoved { .. }.
t=T+..  The stream continues. No re-handshake. No data loss.
        Pre-roam packets still in A's sent map resolve for loss and
        retransmission but feed none of the four fences.
```

**If B moves and stays silent**, it is indistinguishable from a peer that
vanished, and A dies at `R + DEAD_TIMEOUT`. That is correct, and it is
S18's stated positive obligation on the mover.

### 12.2 S11 — contested, with the pending gap

```
t=0     A dialled B. A's basis for B is None. A's address is VALIDATED.
t=T     A roams (B moved, or a test drove it). A's budget ARMS:
        received := len(the roaming packet); sent := 0.
t=T+δ   An Intro for B's static is parked at A. The application calls
        accept().
        A: basis is None ⇒ Err(AcceptError::Stale), guard record reverted,
           connection UNTOUCHED except:
           floor := session.next_counter();
           state := Contested::Pending { floor };
           trace slither::policy "mark" with the floor.
           NO PING. NO TIMER. NO EVENT.
        notified() yields NOTHING.
t=T+δ'  The budget admits the PING (A received enough from B, or A's
        pending output cleared).
           Transmit(PING)  — cwnd-exempt, IN the sent map, IN bytes_in_flight
           state := Armed { floor, armed_at: T+δ', deadline: T+δ'+10s }
           TimerKind::Contested armed at the deadline
           Event(Contested)                      ← in that order
        notified() now yields Notification::Contested.
t=..    A second Intro arrives; accept() again ⇒ Stale.
        TOTAL NO-OP: no second mark, no second PING, deadline UNMOVED.
        notified() yields nothing further.

  branch (a): an ACK covering any counter >= floor arrives before the
              deadline — the PING's own, a PTO retry's, or an ordinary
              Data packet's.
              state := No; Contested disarmed; Event(ContestCleared).
              The connection lives. The refusal stands.
  branch (b): no such ACK by the deadline.
              Event(Closed(TimedOut)) + ToEndpoint::Retired, same drain.
              NOTHING TRANSMITTED. No third notification.
              The static drops to NONE; the parked Intro takes an
              ordinary fresh accept() on the next attempt.
```

### 12.3 S3b — the peer restarts and replaces

```
t=0     B dialled A. A ACCEPTED, so A's basis for B is Some(t0).
t=T     B restarts. It holds nothing. It reconnects.
t=T+ε   B's msg1 arrives at A. A's static for B is LIVE, so it parks as
        an ordinary Intro. The zombie keeps running, untouched.
t=T+δ   The application calls accept() on that Intro.
        t_cand > t0 (B read a fresh wall clock, §5.3) ⇒ REPLACEMENT.
        old connection: Closed(ConnectionLost::Replaced),
                        then ToEndpoint::Retired, same drain.
        new connection: fresh streams, credit, recovery, counters,
                        replay window; basis Some(t_cand).
        The old handle's closed() resolves with Replaced.
        In-flight stream data on the old connection is LOST.
```

Where A had **dialled** instead, the basis is `None`, the accept is
refused `Stale`, and §12.2's probe resolves it within `KEEPALIVE_TIMEOUT`
of the probe — after which the reconnect succeeds. That is S20.

### 12.4 S5 — the two escapes from the 25 s reap

```
(i)   install; no traffic at all
      ⇒ S == R at install, R > S never true, no keepalive ever escapes,
        NOTHING is transmitted, dies at install + 25 s with TimedOut.

(ii)  install; ONE application message in ONE direction
      ⇒ the receiver has R > S, keepalives at S+10s, which puts the
        sender into R > S, and the loop feeds itself. Both live
        indefinitely, with no set_persistent_keepalive call anywhere.

(iii) install; set_persistent_keepalive(Some(10 s)) on ONE side; silence
      ⇒ the beacon fires unconditionally at S+10s. It reaches the peer,
        establishing R > S there, so the peer's passive rule answers at
        once. The pair settles into a 10 s ping-pong. One lost beacon
        costs one interval — the next answer lands at 20 s, 5 s inside
        the deadline. TWO consecutive losses end the connection.
```

---

## §13. Prohibitions — things that look like improvements and are not

Each is declined in `rulings.md` with its reasoning. Re-proposing one
wastes a round.

| Do not | Why | Where |
|---|---|---|
| Put a timestamp in msg2 to fix the `None` basis | **Wire-affecting.** msg2 ≠ 107 B, golden-wire pin red. | ruling 36 |
| Make PING retransmittable | Changes §8.7 for a frame §13.4's probe trains also use. | ruling 41 |
| Clear the contested mark on any post-mark ACK, without a floor | An ACK already in flight at mark time was minted **before** the harvest window closed. | ruling 41 |
| Re-arm the contested deadline on a second refusal | Hands the attacker — who supplies the Intros — a way to postpone the verdict forever. | §7.5, ruling 41 |
| Panic, `debug_assert`, or **clamp** on an out-of-range keepalive | Panic is UB across bubble-ffi to iOS. A clamp reports success while giving a beacon that does not do what was asked. | ruling 44 |
| Name a constant for the persistent-keepalive ceiling | A second place `DEAD_TIMEOUT` is written down is a place it can drift. | ruling 63 |
| Add a third `Contested` event for the unanswered case | The death already arrives as `Closed(TimedOut)`. | ruling 45 |
| Make all keepalive opt-in / delete the beacon knob | Breaks sparse-traffic applications silently; the knob does something no other mechanism does. | rulings 38, 39 |
| Exclude the beacon from the marking set | Unnecessary — the ceiling admits short intervals directly. Arming enables death, never defers it. | ruling 40 |
| Implement PATH_CHALLENGE / PATH_RESPONSE | Two new frame types and a second reset seam. It is §19's deferred lever, not v1. | ruling 13 |
| Add a trace target, or rename one | §18.2 is operator contract; **adding** one is a protocol revision. | ruling 67 |
| Kill a connection, or resolve a verb with an error, on a `send_to` failure | `ENETUNREACH` is the signal that **precedes** a successful roam, not one that follows a dead connection. Trace under `slither::io` and continue. | ruling 49 |
| Build `notified()` over a queue, or claim ahead of the consumer | Rebuilds the unbounded shell queue §10.6 forbids. One slot per kind, one item per `poll`. | rulings 46, 58 |
| Key any new waker map by `StreamId` | A tie-break can invert opener parity. `StreamRef` is the only stable key. | ruling 117 |
| Add an address-keyed map to the endpoint | §17.4: the endpoint tracks **no** per-connection address. `intro_queue.rs`'s doc makes "no second one appears" a review criterion. | §17.4, §6.3 |
| Roam a **closing** or **draining** connection | §15.2: it *"does not roam; never to the triggering packet's source."* | §15.2 |
| Roam on a handshake packet | An accepted initiation **anchors**; that is not roaming. | §7.3 |
| Reset the sent map, `bytes_in_flight`, `pto_count` or `smoothed_rtt` on a roam | §13.6 keeps all of them. Only `min_rtt` is re-seeded and only cwnd/ssthresh reset. | §13.6, §14.6 |

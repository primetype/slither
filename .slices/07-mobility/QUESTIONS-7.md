# QUESTIONS-7 — conflicts, ambiguities and ruling requests for slice 7

Base commit: `b649575`. Planner: slice-7 planning agent.

**Status: COMPLETE, and ten of eighteen are now RULED.**

> **⚠ Round 30 (rulings 168–180) landed while this file was being
> written**, from an adversarial spec audit (`AUDIT-7.md`) run blind to
> the planner. The two documents were written independently and agree on
> six findings — a useful cross-check, and where they differ the ruling
> governs.
>
> | This file | Ruled by | Outcome |
> |---|---|---|
> | **Q1** | **176** | Three-state shape stands; the pending state gains **two** exits the planner did not find |
> | **Q2** | **168, 169, 170** | Budget disarms on return-routability; funded by authenticated **and window-fresh** bytes; **per session** |
> | **Q3** | **172** | **2/2 split ratified** — the planner's uniform-stamp recommendation is superseded, its analysis is not |
> | **Q6** | **174** | `timers.rs`'s order is authoritative; §16.5 gains two relations |
> | **Q9** | **168, 173** | Answered by the disarm predicate; §13.6 grows an explicit roam-reset list |
> | **Q10** | **176** | Answered by "`ContestCleared` only where `Contested` was" |
> | **Q12** | **180** | `FlakyWire::rebind`; **in-flight datagrams abandoned**, contrary to the planner's proposal |
> | **Q14** | **178** | PENDING = membership in the pending tables |
> | **Q16** | **178** | Closed |
> | **Q17** | **175, 177** | §6.9's probe accounting corrected from both directions |
>
> **Unruled and still live: Q5** (§8.5's two "final position" frames) and
> **Q7** (is the passive rule's `S` marking-only). Both carry a stated
> default in CONTRACT-7 §0.
>
> **Still standing as recorded: Q4, Q8, Q11, Q13, Q15, Q18.**

18 items. Written continuously as the planner read.

Working rule 3 applies throughout: where two statements conflict, this file
**reports** the conflict and does not resolve it. Working rule 8 applies:
each "stated construction with an unstated or contradicted scope" is logged
as a question even where the intent seems obvious.

---

## Q-index

**Blocking the dispatch** — each decides something two blind agents would
otherwise decide differently *and consistently*, which is the failure mode
rule 6 exists for. All six want a ruling **before** `CONTRACT-7.md` is
committed, because rule 14 puts the contract in the commit the worktrees
are cut from and a contract amended after dispatch reaches nobody.

| Q | Subject | Shape |
|---|---|---|
| **Q1** | §7.5's contested state is stated as `Option<(floor, deadline)>` but the behaviour needs **three** states; §16.4 names all three | rule 8 / rule 3 |
| **Q3** | Ruling 137's debt: §14.6 assigns **three** of §13.6's four roam fences and leaves the **persistent-congestion walk** unassigned; its closing clause contradicts its own enumeration | rule 8 / rule 3 |
| **Q5** | Ruling 155's debt: §8.5 gives **two** frames "final position" — "PING last" vs the extends-to-end frame. Slice 7 makes the collision routine | rule 3 |
| **Q6** | §16.5 declares its equal-deadline list **exhaustive**; it orders 9 of 28 pairs. **`timers.rs` already picked a total order and slice 5 shipped it unratified** | rule 8 / rule 12 |
| **Q9** | Does the packet that **causes** a roam fund the budget that roam arms? A const-assert settles the *anchor* case and nothing settles the *roam* case | rule 8 |
| **Q14** | **The brief's scope is narrower than ruling 91**: its amendment moved §6.4's PENDING branch into slice 4 too. S4 may already be closed | rule 5 |

**Needed for the contract, lower risk**

| Q | Subject |
|---|---|
| **Q2** | The budget's arming events: is a responder budget-throttled after msg2? What layer do the byte counters measure? (Sub-question 1 is **answered by the spec** and carried into the contract as a pin.) |
| **Q7** | Is the passive keepalive's `S` marking-sends-only? The rule sentence and ruling 40's derivation say different things |
| **Q10** | `Contested`/`ContestCleared` retention: does a second write to an occupied slot take the generation? |
| **Q12** | The fixture cannot express S19; the extension needs a shape and an owner |
| **Q15** | Ruling 117's rationale says "slice 7's replacement installs under a live core"; §5.4 says a replacement is a **fresh** connection |

**Informational / recorded so it is not re-litigated**

| Q | Subject |
|---|---|
| **Q4** | §14.6's `recovery_start = roam instant` has an unstated second consequence; the post-roam stall's interaction with the budget |
| **Q8** | Is `remote_address()`'s total signature safe on every path? (Planner believes yes) |
| **Q11** | §7.5's "marking does three things, in order" predates the budget-pending case and now contradicts §15.4. A **spec edit**, not a behaviour ruling |
| **Q13** | S19's ⚠ CHECK: "we moved" emits **nothing locally**. Pinned in the contract so a test author does not assert an impossible event |
| **Q16** | Ruling 90's recorded-but-unresolved gap: "in-flight outbound initiation" no longer names one set |
| **Q17** | Ruling 43's four directed spec fixes: three verified applied, **ADV-S-4 (§6.9/§17.5 probe accounting) unverified** |
| **Q18** | Where slice 7 sits relative to ruling 130's "§13 and §14 are slice 5's" |

**Numbering.** The highest ruling in `rulings.md` is **167**
(Round 29, slice 6 closed). A slice-7 planning round opens **Round 30** and
numbers from **168**.

---

## Questions

### Q1 — §7.5: the contested state's *stated type* cannot hold the budget-pending case (rule 8)

**Severity: blocking for the contract.** Two statements in §7.5, 20 lines
apart:

- L2326–2327: *"**One mark per connection.** A connection is contested or
  it is not: the state is a single `Option<(probe_floor, deadline)>`,
  never a set."*
- L2438–2442: *"**When the probe cannot be sent…** The deadline is armed
  at the probe's **transmission**, not at the mark, so a probe that
  §7.3's budget will not yet admit leaves the mark **pending** rather
  than failed — the endpoint sends it, and arms, at the first instant the
  budget allows."*

`Option<(probe_floor, deadline)>` has **no representation for
"marked, floor recorded, probe not yet sent, deadline not yet armed."**
The type as stated is exactly two states; the behaviour as stated needs
three (clear / pending / armed). This is the project's most productive
defect shape — a stated construction with a contradicted scope — and it
is **not** cosmetic, because the collapse rule is written against the
type:

- L2328–2331: *"A refusal that lands while the connection is **already**
  contested is **not** a second mark — it leaves the existing floor and
  the existing deadline exactly where they are, and sends no second
  PING."* In the **pending** state there **is** no existing deadline, and
  the second refusal's floor would be *higher* (more seals have happened).
  Does the collapse rule apply in the pending state?

**What the planner believes the intent is** (from L2332–2337's security
rationale — *"re-arming on each refusal would hand the attacker … a way
to postpone the verdict indefinitely"*): collapse applies in **both**
non-clear states; a refusal while pending changes nothing, and in
particular does **not** raise the floor. Raising the floor while pending
would be strictly worse than re-arming, since it would let an attacker
push the floor above anything the peer could yet have acked.

**But `never re-arm` and `never re-float` are different claims and the
spec only makes the first.** Reported, not resolved.

**§16.4 already names the three states — and contradicts §7.5's type
outright.** L4986–4987, in ruling 46's rationale for splitting
`ContestCleared` out of `Contested { under_probe: bool }`: *"the three
real states (**marked-pending, probing, cleared**) do not map onto one
bool at all."* So the spec knows there are three. §7.5's
`Option<(probe_floor, deadline)>` is the two-state formulation, and
§16.5's timer prose (L5065–5072) is written against the two-state one too
(*"it is armed at the probe's transmission — never at the mark, which may
wait on §7.3's budget"* — correct, but silent on what holds the mark
meanwhile). **This is rule 3's pattern: the prose (§16.4) holds the
correct intent, the code-like rule (§7.5's type) holds the bug.** The
planner is reporting it rather than picking.

**Ruling requested:** ratify the three-state shape and state the collapse
rule over it. Proposed contract text if ratified — CONTRACT-7 §3 pins:

```rust
enum Contested {
    /// Not contested.
    No,
    /// Marked; floor recorded; probe not yet admitted by §7.3's budget.
    Pending { floor: u64 },
    /// Probe transmitted at `armed_at`; verdict due at `deadline`.
    Armed { floor: u64, deadline: Instant },
}
```

with: a refusal in `Pending` or `Armed` is a **no-op in every field** (no
new floor, no PING, no deadline).

---

### Q2 — §7.3 + §7.5: the contested probe can only ever be budget-blocked *after a roam*, and the spec never says so

**Severity: informational, but it decides how S11's pending case is
tested (working rule 9).**

§7.3 L1914–1919 arms the budget *"whenever a session's endpoint address
changes (a roam) **or** is first anchored from a msg1 source."* Those are
the only two arming events stated.

The contested mark is taken only where `replacement_basis == None`
(§7.5 L2306–2308), and the basis is `None` **only where we dialled**
(S3c, STORIES L92–95). A connection we dialled has its address from the
application's `connect(addr)` — **neither a roam nor a msg1 anchor** — so
its budget is never armed at install.

**Consequence:** `Contested::Pending` — the whole *"a connection may not
be killed by a question that was never asked"* branch, and the
*"notification at probe transmission, not at marking"* separation that
story S11 is explicit about — is reachable **only on a dialled connection
that has since roamed**. A test that marks a fresh dialled connection
contested will *never* observe the pending state, and a test author who
does not know this will write a "notification fires at transmission" test
that passes against an implementation that fires it at marking. That is
precisely rule 9's degenerate-case failure.

**The deduction is confirmed by the spec, in a section §7.5 does not
cross-reference.** §16.4 L4970–4972 states it outright: *"a probe §7.3's
amplification budget will not yet admit leaves the mark *pending*, which
is **reachable exactly when the connection has just roamed to an
unvalidated address**, not an exotic corner for a mobility-first
protocol."* And App. B L6224–6225 makes it a test instruction: *"Drive
the mark-pending case above — **roam to an unvalidated address so §7.3's
budget holds the probe** —."*

So Q2's first bullet is **answered by the spec** and needs no ruling; it
needs to be **in the contract**, because neither §7.5 nor §6.4 says it and
a test author working from those two sections alone will not find it.
The remaining sub-questions do need answers:

**Questions:**

1. **[Answered — carried to CONTRACT-7 §3 as a pin, not a question.]**
   An application-supplied `connect()` address is **validated**; the
   budget is not armed there. §16.4 L4970–4972 and App. B L6224–6225
   settle it. §7.3's own list settles it only by omission, which is why
   it is logged.
2. Confirm the corollary: is a **responder's** connection budget-armed at
   install? §7.3 says an accepted initiation *"anchors at its msg1
   source"* and that this arms the budget — so **yes**, every inbound
   connection starts unvalidated with a 3× budget funded by the
   ≥ `INIT_PACKET_LEN` (196 B) msg1 that anchored it. That gives 588 B of
   send credit, and `RESP_PACKET_LEN` is 107 B, so msg2 fits — but the
   margin is 481 B, **less than one max-size Data packet**. Is that
   intended? It means a responder that wants to send immediately after
   msg2 is budget-throttled until the peer's first Data arrives. This
   looks intended (it is exactly QUIC's posture) but it is **nowhere
   stated**, and it is the difference between "the responder can push"
   and "the responder must wait a round trip."
3. Do the two counters (bytes-sent, authenticated-bytes-received) count
   **datagram** bytes (UDP payload, i.e. including the slither header and
   AEAD tag) or **plaintext** bytes? §7.3 says *"total bytes sent to the
   address"* and *"total bytes received from it and authenticated"*,
   which reads as on-the-wire datagram lengths in both directions, but
   the phrase *"and authenticated"* attaches to a packet-level property
   while *"bytes"* does not name a layer. The 3× ratio is only QUIC's
   ratio if both are datagram bytes. **Report, not resolve.**

---


### Q3 — ruling 137's carried debt: §14.6 assigns **three** of the four roam fences and leaves the fourth silent

**Severity: blocking for the implementer.** This is the debt the brief
names. Working it out:

§13.6 L3808–3811 and §14.6 L3953–3955 both list the **four** things a
pre-roam packet must not feed:

1. a congestion event
2. the persistent-congestion walk
3. an RTT sample
4. `app_limited` window growth

§14.6's ruling-137 amendment (L3957–3967) says, verbatim: *"One marker
cannot serve those four fences and the text must not be read as saying it
does. The recovery-period marker serves **the congestion event and
`app_limited` growth**, because §14.3 already gates both on it. It
**cannot** serve **the RTT fence** … The mechanism that works is …
**path-generation stamping**. `SentPacket` therefore carries a `u32` path
generation from slice 5 onward, held at 0 until roaming exists, and
**§13.6's fences read that stamp** rather than the recovery marker."*

**The persistent-congestion walk (fence 2) is assigned to neither.** It
is named in the four, then never mentioned again. And the closing clause
is a blanket — *"§13.6's fences read that stamp"*, all of them — which
contradicts the two sentences before it that hand two fences to the
recovery marker. This is rule 8's shape exactly, and rule 3's: the
enumeration and the closing clause disagree.

**Independent check, per rule 11 — does the mechanism exist where the
rationale says it does?**

- §14.3 L3862–3867: *"the same `sent_time ≤ recovery_start` test gates
  both **the event** and **the growth**."* Confirmed — the marker really
  does serve fences 1 and 4, and only those two.
- §14.4 L3886: *"Packets sent before a roam are **excluded from the
  walk** (§13.6, §14.6)."* The obligation is stated; **no mechanism is
  named**, and §14.3's marker demonstrably does not reach the walk. So
  fence 2 must be the **stamp**, by elimination — but no text says so.

**Planner's answer to the brief's question — "can one field serve all
four?" Yes, and it should, and here is the argument that the split is
actively wrong:**

The marker test is `sent_time ≤ recovery_start` with `recovery_start`
set to the roam instant. The stamp test is `pkt.path != current_path`.
These are equivalent **only if no packet is sent at exactly the roam
instant.** On tokio's paused clock — the harness this entire project
mandates (`CLAUDE.md`, architecture invariants) — `Instant::now()` does
not advance between calls, so **pre-roam and post-roam packets routinely
carry the identical `sent_time`.** Under `≤`, a packet sent *after* the
roam but in the same virtual instant is fenced as though it were
pre-roam: its loss fires no congestion event and its ACK grows no window,
**on the new path, indefinitely for that instant's flight**. The stamp
has no such failure — the generation is bumped at the roam and every
subsequent send carries the new value regardless of clock resolution.

> **⚠ RULED — 172. The recommendation below is superseded; the analysis
> is not.** The **2/2 split is ratified**: `recovery_start` fences the
> congestion event and `app_limited`; `path_gen` fences the RTT sample
> and the persistent-congestion walk. The planner's paused-clock
> objection is answered for *pre-roam* packets — `recovery_start` is
> never cleared (ruling 139(b)) and moves monotonically forward, so a
> pre-roam packet cannot escape by resolving late. **The other half
> survives**: a *post-roam* packet sent in the same virtual instant as
> the roam has `time_sent == recovery_start` and *is* fenced. That is now
> a test-authoring instruction in CONTRACT-7 §3.4, not a reopened
> question. Ruling 172 also corrects the framing: ruling 137's fences are
> **not open debt** — slice 5 closed the design, slice 7 owes wiring.

**Ruling requested.** Recommend: **all four fences read the `u32` stamp**;
`recovery_start` is still set to the roam instant (§14.6 says *"set to
the roam instant — not cleared"*, and that clause does other work — it
suppresses a *legitimate* new-path congestion cut for one episode, which
is a separate, deliberate behaviour and must not be dropped by accident),
but it is **no longer the fence for any of the four**. That is a change to
§14.6's stated allocation and therefore needs a ruling; the planner will
not resolve it.

**If the ruling goes the other way** (marker keeps fences 1 and 4), the
contract must say so precisely and the test authors must be told that the
equal-instant case is *expected* to fence post-roam packets — because
otherwise a paused-clock test written from §13.6's prose will fail against
a conformant implementation, and the integrator will read it as a bug.

---

### Q4 — §14.6's `recovery_start = roam instant` has a second, unstated consequence

**Severity: low, but it is a test-visible behaviour nobody has written
down.** §14.6 L3951–3952: *"The recovery-period marker is **set to the
roam instant — not cleared**."* Combined with §14.3's one-cut rule, this
means: **for the first loss episode after a roam involving any packet
sent at or before the roam instant, the fresh `INITIAL_WINDOW` takes no
cut, and ACKs of that flight grow nothing.** That is intended (it is the
fence). But the rationale in §14.6 justifies the marker only as a fence
against *pre-roam* flight; it never states the flip side, which is that
the marker also cannot be *cleared* by a post-roam congestion event
earlier than itself — trivially true, since time moves forward.

The question is narrower and real: **does the roam reset `pto_count` and
the loss-detection timer state?** §13.6 says the sent map is kept and
*"loss detection and PTO continue undisturbed"* — so **no**. But §13.6
also says *"immediately after a roam, `bytes_in_flight` may exceed the
fresh initial window; the admission gate then blocks new sends until
old-path packets are acknowledged or declared lost — a bounded stall of
at most one loss-detection/PTO cycle, kept probeable by the PTO exemption
**within §7.3's budget**."*

That last clause is the interaction the brief's debt 3 is about, and it
has a sharp edge: **immediately after a roam the budget is 3× the bytes
of the single packet that caused the roam.** A max-size Data packet that
roams us funds ~3× its own size. If the stall-breaking PTO probe plus any
pending ACK exceeds that, the connection is *both* cwnd-stalled and
budget-stalled, and the only thing that clears it is another authenticated
packet from the peer. §7.3 L1932–1935 argues no deadlock is possible
because *"any ack-eliciting output we aim at the address arms the death
clock by itself, even where nothing marking is sent"* — i.e. the failure
mode is death, not hang. **Is that the intended outcome, or should the
budget have a floor (e.g. always admit at least one MTU)?** RFC 9000
§8.2.1 has exactly this discussion and permits sending a packet that
would exceed the limit when the limit would otherwise prevent any send.
slither's §7.3 says *"MUST NOT exceed"* with no such escape.
**Report, not resolve** — but note that "3 × 1 packet ≥ 1 packet" holds
for equal sizes, so the practical bite is only when the roaming packet is
small (a keepalive is the empty plaintext, the *smallest* packet in the
protocol) and our response is large. **A keepalive-driven roam — which is
precisely S18's mechanism, "the mover must send and the keepalive is what
does it" — is the minimum-budget case in the protocol.**

---

### Q5 — ruling 155's carried debt: §8.5 gives **two** frames "final position" and slice 7 makes the collision live

**Severity: blocking for the implementer; it decides a wire-visible
packing outcome.** §8.5 L2828–2832, one paragraph, two claims:

> *"Within a packet the sender packs in this order: the ACK first (if
> owed), then control frames (credit grants, RESET_STREAM, CLOSE), then
> STREAM and DATAGRAM fill, **then PING last** if a probe still owes
> ack-eliciting content. **At most one extends-to-end frame (¬LEN STREAM,
> or `0x30` DATAGRAM) per packet, in final position.**"*

A `0x30` DATAGRAM or a ¬LEN STREAM runs to the end of the packet by
definition — there is no length field, the decoder takes the remainder.
**A PING cannot be packed after one.** So on any packet that both owes a
probe and has an extends-to-end frame available, §8.5 requires two
different frames to be last. The spec states no precedence.

The three conformant resolutions are not equivalent and a test can tell
them apart:

- **(a) PING wins, the fill uses the LEN form.** The DATAGRAM is emitted
  as `0x31` (explicit length) and the PING follows. Costs the varint
  length prefix; a *maximum-size* datagram then no longer fits and is
  deferred to the next packet. **This is exactly ruling 155's documented
  bias, and it now fires on every PTO probe and every contested probe,
  not only on control-frame packets.**
- **(b) The extends-to-end frame wins, the PING moves to its own
  packet.** Costs one extra datagram per probe; the probe is then a
  minimal packet and cheap. Arguably better for the *probe*, since a
  probe that rides a full data packet is more likely to be lost to a
  size-related drop.
- **(c) The PING is simply not packed on that packet** and the probe is
  considered satisfied by the ack-eliciting STREAM/DATAGRAM content that
  is there. §8.5's own conditional — *"if a probe still owes
  ack-eliciting content"* — reads as licensing this: a packet carrying a
  STREAM frame already elicits an ACK, so nothing is owed. But the
  **contested probe is not a PTO probe**, and this is where (c) becomes
  dangerous: §7.5 says the mark *"sends an ack-eliciting **PING**"* and
  the deadline arms *"at that PING's transmission"*. If (c) lets a
  STREAM-bearing packet stand in for the PING, then what arms the
  `Contested` timer and what fires `Notification::Contested`? §7.5
  L2312–2315 hedges — *"ordinarily the first packet at or above that
  floor, though **the floor is what binds** and it is recorded at the
  mark whether or not the PING is the very next seal"* — which suggests
  the floor is the real object and the PING is a convenience. But §16.4
  and §15.4 both bind the notification and the deadline to *"the probe's
  transmission"* as a single identified event.

**Question 5a (packing precedence):** which of (a)/(b)/(c)? A ruling is
needed; the planner will not pick, and an unpicked choice guarantees the
blind implementer and the blind test author disagree.

**Question 5b (what "the probe's transmission" means):** is the
`Contested` deadline armed by *the packet carrying the PING*, or by *the
first packet sealed at or above the probe floor*, whatever it carries?
These coincide under (a) and (b) and diverge under (c). §7.5's own
parenthetical says the floor binds; §16.5's timer prose says the PING's
transmission arms.

**Answer to the brief's "does slice 7 worsen ruling 155's bias
measurably?"** — **Yes under (a), no under (b) or (c).** Under (a) the
bias's trigger set grows from "packets carrying control frames" to
"packets carrying control frames **or owing a probe**", and probes are
emitted on a PTO cadence on any lossy path, plus once per contested mark.
Under (b) the bias is unchanged and the cost is one extra datagram per
probe. §8.5's packing order otherwise **still holds** — slice 7 adds no
new frame type (PING is already `0x01` in §8.3's table and already used
by §13.4), and the keepalive is the **empty plaintext** (§3.4, §7.5
L2053), which *"bypass[es] the frame layer"* entirely and therefore
exerts no packing pressure at all.

---

### Q6 — §16.5's equal-deadline list claims to be exhaustive; slice 7 makes at least four unordered pairs constructible

**Severity: blocking for the test authors — an unordered pair is a
coin-flip disagreement between two blind agents.**

§16.5 L5086–5090: *"**Equal-deadline priorities** (normative). … This
list is **exhaustive**: every pair of deadlines that can fall on one
instant is ordered here, because §16.4 makes generation order normative
and an unordered pair would make that claim hollow exactly where two
timers collide."*

The connection timer table is `Keepalive`, `PersistentKeepalive`,
`Liveness`, `Loss`, `Pto`, `AckDelay`, `CloseLinger`, `Contested` — eight
names, 28 pairs. What L5096–5106 actually orders:

| Pair | Ordered? |
|---|---|
| `Loss` vs `Pto` | yes — loss beats PTO, exactly one fires |
| `Liveness` / `CloseLinger` / `Contested` vs `Keepalive` | yes — "teardown collection … precedes keepalive evaluation" |
| `Liveness` vs `CloseLinger` vs `Contested` | yes — that internal order, and "`Liveness` beating `Contested` … is the harmless ordering" |
| `AckDelay` vs `Loss` / `Pto` | yes — AckDelay after |
| `PersistentKeepalive` vs everything | yes — "evaluated last" |
| **`Keepalive` vs `Loss`** | **no** |
| **`Keepalive` vs `Pto`** | **no** |
| **`Keepalive` vs `AckDelay`** | **no** |
| **`Liveness` / `CloseLinger` / `Contested` vs `Loss` / `Pto` / `AckDelay`** | **no** |

Before slice 7, `Keepalive`, `PersistentKeepalive` and `Contested` did
not exist in the implementation, so the gap was not constructible. **Slice
7 is the slice that constructs it**, and the collisions are not exotic:
`KEEPALIVE_TIMEOUT` is 10 s and a PTO on a stalled path lands wherever it
lands; on a paused clock a test *chooses* the collision.

**It is observable.** The passive keepalive is a **marking** send of the
empty plaintext; the PTO probe is a `seal_quiet` PING. Because `S` in the
passive rule counts **marking sends only** (§7.5 L2117), a PTO probe
firing first does **not** suppress the keepalive — so both packets go out
and only their **order on the wire** differs. `Tap` sees sealed datagrams
in order, so a paused-clock test distinguishes the two, and two blind
agents will pick differently.

**The code has already picked, and slice 5 shipped it unratified.**
Working rule 11 says check the mechanism against the code, so the planner
opened `src/core/connection/timers.rs` at `b649575`:

```rust
// src/core/connection/timers.rs:32-50 — declaration order IS the priority,
// via the derived Ord, and Due::iter walks TimerKind::ALL filtered.
pub(crate) enum TimerKind {
    Liveness, CloseLinger, Contested,        // teardown collection
    Loss, Pto, AckDelay,                     // recovery evaluation
    Keepalive, PersistentKeepalive,          // emission, beacon last
}
```

and `timers.rs:54–56` calls the array *"Every timer, in ruling 76's order.
**Exhaustive by ratification** — §16.5 says so in as many words, which is
why this array is written out rather than derived."*

So slice 5 **resolved every unordered pair in the table above**, in
exactly the total order the stated principle implies (teardown → recovery
evaluation → keepalive → beacon), and attributed it to a ruling that does
not contain it. The three slice-7 timers were declared then and armed by
nothing, so nothing has ever exercised the choice.

**This is the pattern working rule 12 warns about**: the array's own doc
comment is a *true* statement about §16.5's exhaustiveness *claim*, used
to justify an ordering §16.5 does not state. Nobody has been wrong yet —
the order looks right — but it is unratified, it becomes observable for
the first time in this slice, and a test author reading §16.5 cannot
derive it.

**Ruling requested:** ratify `timers.rs`'s total order into §16.5, so the
spec and the code agree and the test author has a source. Nothing in the
code needs to change if the answer is "yes".

Note this is *also* a rule-8 instance in a section that **explicitly
claims exhaustiveness for itself** — ruling 76's own words. Ruling 71 and
ruling 95 are the same shape and both were found "by opening the file",
which §16.4 L4959 records.

---

### Q7 — §7.5's passive keepalive rule: `S` is "marking sends only", but the rule is stated over "has not sent"

**Severity: medium — it is a one-line divergence with a 10 s-visible
consequence.**

Two statements of the same rule:

- §7.5 L2056–2057 (the rule): *"Passive rule: a side that **has received
  since it last sent**, and **has not sent** for `KEEPALIVE_TIMEOUT`,
  sends a keepalive."* — "sent", unqualified, twice.
- §7.5 L2116–2119 (ruling 40's derivation, which the section says is
  *"retained here"* as load-bearing proof): *"Let `I` be the configured
  interval, `S` = **`last_send` (marking sends only)**, and `R` =
  `last_authenticated_recv`."*

Under the unqualified reading, a `seal_quiet` send — a PTO probe, a
credit frame, a retransmission, the contested probe's own PING — resets
`S` and **suppresses the keepalive for another 10 s**. Under the
qualified reading it does not. §7.4's whole point is that `seal_quiet`
output is *liveness-neutral*, which argues hard for the qualified
reading, and ruling 40's arithmetic depends on it (the beacon derivation
computes `S + I` from marking sends).

**But the consequence differs by more than tidiness.** A connection under
sustained PTO retry has quiet-but-ack-eliciting output continuously. Under
the unqualified reading it emits **no keepalive at all** while probing, so
the peer — which has been receiving PINGs and therefore has `R > S` —
keeps answering with keepalives, and the asymmetry is stable. Under the
qualified reading both sides keepalive on their own 10 s cadence
regardless. The wire traces differ from the first probe onward.

**Report, not resolve.** The planner's reading is the qualified one
(`S` = last **marking** send), because §7.4 and ruling 40 both require it
and §7.5's one-line statement is the informal restatement. Rule 3 says
the prose usually holds the intent — here the *derivation* is the prose
and the *rule sentence* is the code-like one, so the usual polarity is
inverted and the planner is not confident. It must be pinned in
CONTRACT-7 either way, because it is exactly the kind of thing two blind
agents resolve differently and consistently.

---

### Q8 — is `remote_address()` on a `connect()`-created connection defined before install?

**Severity: low, but it is an `unwrap`-shaped hole in a signature that
cannot express failure.**

§16.2 L4206: `pub fn remote_address(&self) -> SocketAddr;` — total, no
`Option`, no `Result`. §16.9 (via ruling 95) makes a `connect()`-created
connection **writable before install**, and §16.4 makes `is_established()`
false there. The address is known (the application supplied it to
`connect`), so the total signature is satisfiable — but that is a fact
about the *dialled* path only.

Is there any path on which a `Connection` handle exists and no address
does? §17.4 L5515–5518 says *"the endpoint tracks no per-connection
address (the connection core owns its own endpoint address, §7.3)"*, so
the core must be given one at construction. For `accept()` it is the msg1
source. For `connect()` it is the argument. The planner believes the
signature is safe on both paths and is flagging it only so the contract
can say so explicitly rather than leaving the implementer to discover an
`Option` it then cannot return.

---

### Q9 — §7.3's budget: what is the unit of "reset at each such address change", and does the *anchoring* packet fund the budget it arms?

**Severity: medium — it changes whether a responder can send msg2 at
all.**

§7.3 L1915–1920: *"whenever a session's endpoint address changes (a roam)
or is first anchored from a msg1 source, the address is **unvalidated**
and a send-side budget arms — total bytes sent to the address MUST NOT
exceed `AMPLIFICATION_FACTOR` (= 3) × total bytes **received from it and
authenticated** on this session, **both counters resetting at each such
address change**."*

If **both** counters reset **at** the address change, then at the instant
of arming, received = 0, so the budget is 0 and **nothing may be sent** —
including the msg2 that the anchoring initiation is waiting for, and
including any response to the roaming packet. The budget could then never
open, because opening it requires receiving, and receiving is not
something we can cause.

The reading that works is that the packet **causing** the change is
counted into the fresh received-counter — i.e. reset, *then* credit the
triggering packet. §7.3 supports this obliquely for the anchor case:
*"the anchoring initiation qualifies, its handshake tail tags having
verified at admission"* (L1921–1923) — that sentence exists precisely to
say the msg1 funds the budget. **It says nothing equivalent for the roam
case**, and the roam case is the one slice 7 builds. Rule 8: a stated
construction (the anchoring packet counts) with an unstated scope (does
the roaming packet?).

**This is not academic.** S18's mechanism is a **keepalive-driven roam**
(*"the mover must send — and the keepalive is what does it"*), and a
keepalive is the **empty plaintext, a 30-byte datagram** — the smallest
packet the protocol has. If the roaming keepalive funds the budget, we
get 90 bytes of credit; a pure ACK may or may not fit. If it does not
fund it, we get **zero** and the connection cannot answer the roam at
all until the peer sends again unprompted — and the peer, having just
keepalived, will not send again for another `KEEPALIVE_TIMEOUT`. The two
readings differ by whether S18 works at all on an otherwise-idle link.

**A const-assert in the shipped code settles the anchor half and leaves
the roam half open.** `src/constants.rs:619–621`, verbatim:

```rust
// §7.3's budget must admit at least one response to one initiation, or a
// responder could never answer an unvalidated address at all.
const _: () = assert!(AMPLIFICATION_FACTOR as usize * INIT_PACKET_LEN >= RESP_PACKET_LEN);
```

`3 × 196 ≥ 107`. That assertion is **only meaningful under the
reset-then-credit reading** — under reset-to-zero the responder has a
0-byte budget and the inequality is about nothing. So the anchor case is
settled by the code, and the comment states the intent in the words the
spec did not use: *"or a responder could never answer an unvalidated
address at all."*

**The roam case has no such assert and no such sentence**, and the roam
case is what slice 7 builds. The symmetric statement would be
`AMPLIFICATION_FACTOR × 30 ≥ (one pure ACK datagram)` for a
keepalive-driven roam — 90 bytes of credit against a `DATA_HEADER_LEN`
(14) + ACK frame + 16-byte tag reply, which fits, but only just, and
nothing anywhere states that it must.

**Ruling requested**, explicitly, with the roam case named: does the
packet that causes a roam fund the budget that roam arms? And if so,
should the roam case get its own const-assert beside the anchor one, so
the two cannot drift?

---

### Q10 — `Notification` retention: "one slot per kind" vs. `Contested`/`ContestCleared` ordering

**Severity: medium — a test author will write this and needs the exact
answer.**

§16.2 L4415–4423: *"**Retention is one slot per kind** … `Contested` and
`ContestCleared` are distinct kinds, and ruling 41's collapse means at
most one mark exists per connection at a time, so at most one of each can
ever be pending. Pending notifications of different kinds are handed over
in **generation order** (§16.4's ordering rule)."*

Take: probe transmitted at *t*, ACK clears it at *t*+2 s, application
calls `notified()` twice at *t*+3 s. It gets `Contested` then
`ContestCleared`. Fine.

Now: the connection is contested **twice over its life** — mark, clear,
mark again — and the application never calls `notified()` until the end.
"One slot per kind" means the second `Contested` overwrites (or is
dropped by) the first, and the single `ContestCleared` sits between them
in generation order that no longer exists. The application then sees
`Contested`, `ContestCleared` — or `ContestCleared`, `Contested` —
depending on whether the second mark's generation replaces the slot's
generation. **§16.2 does not say whether a second write to an occupied
slot updates that slot's generation.**

For `AddressMoved` the spec answers exactly this (*"keeps the oldest
unclaimed `from` and the newest `to`"* — a merge rule) and App. B tests
it. For `Contested`/`ContestCleared` there is no merge rule because the
payload is empty, so only the **generation** can change, and nothing says
whether it does.

**Concretely, the two candidate answers:**
1. **Slot generation is the first write** (arrival order preserved): the
   application sees `Contested`, `ContestCleared` — which correctly
   reports "this connection has been contested and a contest cleared",
   but a *second* unanswered mark would be indistinguishable from a
   cleared first one.
2. **Slot generation is the latest write**: the application sees
   `ContestCleared`, `Contested` — reading as "contested **now**", which
   is the state a reconnect scheduler actually wants.

The planner reads §16.2's stated purpose (*"a reconnect scheduler needs
set, cleared, and `Closed`, no more"*, §16.4 L4989) as favouring (2), and
§16.2's `AddressMoved` merge rule — which deliberately preserves the
oldest `from` — as favouring (1). **Report, not resolve.**

This may be unreachable: a mark can only be taken on a `None`-basis
connection, the mark's failure kills the connection, and its success
leaves the connection alive and still `None`-basis — so a **second mark
is constructible** (another refusal after a clear). It is not exotic.

---

### Q11 — §15.4's contested row says the probe is sent "at the mark — or at the first instant §7.3's budget admits it"; §7.5 says the mark itself sends it

**Severity: low; a wording tension already resolved elsewhere, logged for
rule 4 (grep the rationale, not the token).**

§7.5 L2308–2315 still reads *"Marking does three things, **in order**: it
records the mark's probe floor … it sends an ack-eliciting PING … and it
arms a `KEEPALIVE_TIMEOUT` deadline at that PING's transmission"* — a
formulation written before the budget-pending case at L2438–2442 was
added. §15.4 L4093 has the corrected form. The list-of-three is the
sentence a reader reaches first and it is the one that is wrong.

No ruling needed for behaviour (§15.4, §16.4 and App. B all agree). A
**spec edit** is wanted so the section does not contradict itself, and it
is exactly the shape rule 4 exists for — a value was changed and the prose
arguing the old position was left standing.

---

### Q12 — the fixture cannot express S19, and the extension has no owner yet

**Severity: blocking for the slice's acceptance criteria.** See PLAN-7 §3
for the full analysis. Summarised as a question: **S19 ("*our* address
changes / the NAT rebinds") is not expressible on `FlakyWire` as it
stands**, and working rule 13 says the fixture bounds the coverage. The
plan proposes an extension and names an owner; the maintainer should
confirm the shape before dispatch, because it is a `testutil` change and
`testutil` is on the public-ish surface that `cargo doc` gates.

---

### Q13 — S19's ⚠ CHECK: "we moved" vs "they moved" are indistinguishable, deliberately

**Severity: informational — recording that slice 7 ships the "today"
behaviour.** STORIES L317–320: *"the amplification/roaming interaction was
reviewed and the probe is not an address-steering reflector. But an
application-visible story for 'we moved' versus 'they moved' may want
distinct handling; **today both surface as address changes**."*

Slice 7 ships `Notification::AddressMoved { from, to }` for both. Note
the asymmetry that makes them genuinely different events at the core:

- **They moved**: *our* connection sees a source-address change and emits
  `AddressMoved`. `remote_address()` changes.
- **We moved**: our address changed; **our own connection emits nothing**
  and `remote_address()` is unchanged. The *peer* emits `AddressMoved`.

So on the "we moved" side there is **no notification at all**, and S19's
acceptance is entirely peer-side. A test author must be told this or they
will assert a local `AddressMoved` that can never fire. **Pinned in
CONTRACT-7 §5, not left to the story text.**

---

### Q14 — **my brief's scope statement is narrower than ruling 91 actually is** (working rule 5)

**Severity: blocking — it decides whether an implementer is dispatched to
build something that already exists.**

The slice-7 brief says: *"§5.4 / §6.4 / §6.7–6.8 replacement + tie-break +
restart (**§6.5 and §6.6 moved to slice 4 by ruling 91**)."*

Ruling 91 has an **amendment** (`rulings.md` L1962–1981) that the brief
does not reflect: *"**§6.4's PENDING branch comes too.** … Both sides of
the branch are therefore in slice 4, including that the winner-side
`Stale` is **the one refusal that keeps its §17.1 guard record**
(§6.4:1401-1406) where every other reverts it."*

So §6.4's **PENDING** branch — both orderings, the
`Err(ConnectError::AlreadyConnected)` resolution of the cancelled
pending, and the winner-side guard record — is **slice 4's**, not slice
7's. What remains of §6.4 for slice 7 is the **§16.1 guard / LIVE branch**:
basis `Some(t)` + strictly-newer ⇒ replace with `Replaced`; otherwise
`Stale`, and against a `None` basis, mark contested.

Working rule 5 says to report a brief that looks wrong rather than
following it. **Confirm the scope**, because two of this slice's eight
stories (S4 and half of S3) sit on that boundary:

- **S4** (simultaneous open) is anchored to *"§6.6, §6.7 (ruling 35)"* —
  §6.6 is slice 4's and §6.7 is the comparison rule §6.6 and §6.4 both
  apply. If both routes to it landed in slice 4, **S4 may already be
  closed**, and slice 7's obligation is to *verify* it rather than build
  it. App. B's *"Convergence, the regression this ruling exists for"*
  (L5978–5984) is the test that decides.
- **S3b/S3c** sit on §6.4's **LIVE** branch, which is unambiguously slice
  7's.

**The planner's proposal, for ratification:** slice 7 owns §6.4's LIVE
branch and §6.7's *consequences* (stream parity survives a replacement),
and **treats S4 as an acceptance-verification story, not a build story** —
its tests are written and run, and if they pass against `b649575` with no
implementation change, that is the correct outcome and must not be read
as the test being weak. Rule 9 applies with force here: the S4 tests must
be written so a *broken* implementation fails them (both static
orderings, data flowing in both directions, agreeing parity — App. B
L5978–5984 says exactly this and says a test exercising one ordering *"is
not sufficient"*).

---

### Q15 — ruling 117's rationale names "slice 7's replacement" installing under a live core; §5.4 says a replacement is a **fresh connection**

**Severity: medium. This is a rule-11 check on the maintainer's own text,
and it may be nothing — but the two readings imply different code.**

`rulings.md` L2878–2882 (ruling 116) and L2914–2919 (ruling 117), on why
the shell's waker maps must key by `StreamRef` and not `StreamId`:

> *"§6.7's tie-break and **slice 7's replacement** both install a session
> *underneath a connection core that already holds queued sends*, and a
> `StreamId` is fixed by an opener parity that a tie-break can
> **invert**."*

For the **tie-break** this is exactly right and the mechanism plainly
exists: a `connect()`-created core is writable before install (§16.9),
loses the tie-break, and the tie-break's `Install` puts a *responder*
session under it — inverting parity under live `StreamRef`s.

For **slice 7's replacement** the spec says the opposite. §5.4 L820–823:
*"Every completed handshake therefore establishes a **fresh connection
with fresh transport state on both sides** — sessions and connections are
1:1, and **no stream, flow-control, recovery, or congestion state ever
crosses a handshake**."* And S3's accepts: *"the new connection is
independent, **with fresh stream state**."* A replacement does **not**
install under a core holding queued sends; it builds a new core and kills
the old one with `Replaced`.

**Ruling 117's conclusion is correct on the tie-break alone**, so nothing
downstream is at risk. But if the implementer reads the rationale as
normative it will look for — or build — a re-install-under-a-live-core
path for the replacement that §5.4 forbids. **Please confirm the
rationale's "slice 7's replacement" clause is an error, so
CONTRACT-7 can state plainly that a replacement is a fresh core.**

This is precisely the defect class working rule 11 names, and its
sub-clause: *"a rationale is not reviewed by the act of ratifying its
rule."*

---

### Q16 — ruling 90's recorded-but-unresolved gap: "in-flight outbound initiation" no longer names one set

**Severity: low for slice 7, but it is an explicitly *unresolved* debt and
this is the slice that inherits it.**

`rulings.md` L1988–1994: *"`mint_pending` without `start_attempt` is a
pending with **no initiation in flight**. §6.5 speaks of 'in-flight
outbound initiations' and §17.4 of 'the pending tables' dialled
addresses' — those named the same set before ruling 90 and no longer do,
and the same ambiguity reaches §6.4's PENDING branch ('if an in-flight
outbound initiation exists')."*

§17.4 L5512–5514 is one of slice 7's reading list: *"the `static →
connection` map … plus the pending tables' dialled addresses (§17.3),
which are §6.5's hint set."* If §6.4's PENDING branch is slice 4's
(Q14), this gap is slice 4's residue and slice 7 only needs to know the
answer, not fix it. Flagging it because §5.4's **PENDING** row is one of
the three states slice 7's LIVE-branch code must distinguish, and
"PENDING" has to mean something precise for the LIVE test to be the right
test.

---

### Q17 — three directed items from ruling 43's batch: two verified applied, one unverified

Rule 11 says check the mechanism against the file. Ruling 43's round
directed four spec fixes without ruling numbers (`rulings.md`
L553–558). Slice 7 builds on all four sections, so the planner checked:

| Item | Claim | Verified? |
|---|---|---|
| ADV-S-2 | §15.4's teardown matrix misdescribes the contested death | **Applied.** §15.4 L4093 now carries the full contested row with the transmission-armed deadline and the asymmetric peer view. |
| ADV-S-3 | §16.5's timer table has no contested entry | **Applied.** §16.5 L5064–5072 lists `Contested` and specifies its arming. |
| ADV-S-5 | §7.3's exemption list is stale, pre-ruling-36 | **Applied.** §7.3 L1912 now reads *"(PTO probes, the contested-connection probe, pure ACKs, CLOSE, keepalives)"*. |
| **ADV-S-4** | §6.9 / §17.5 cost accounting for the probe | **Not verified by the planner** — §6.9 (L1713–1841) and §17.5 were not read in full within this slice's reading budget. Flagging rather than asserting, per rule 7's spirit. If the probe is missing from §6.9's accounting the correction belongs in this slice's round, since slice 7 is what makes the probe real. |

---

### Q18 — where slice 7 sits relative to ruling 130's "§13 and §14 are slice 5's"

**Severity: informational, resolved by the planner but recorded so the
implementer does not re-litigate it.**

`rulings.md` L3337–3348 (ruling 130): *"§13 and §14 are **slice 5's**. Six
texts say slice 7 and they all descend from one."* Read flat, that would
put §13.6 and §14.6 — both on this slice's reading list — out of scope.

They are not, and ruling 137 (L3480–3483) settles it: the `u32` path
generation on `SentPacket` is *"documented as **slice 7's**, asserted 0 in
slice 5."* The correct division:

- **Slice 5 owns** §13 and §14's machinery — the sent map, loss detection,
  PTO, NewReno, the recovery period, persistent congestion, the admission
  gate.
- **Slice 7 owns the roam *seam* through them** — §13.6 (what is kept and
  what is fenced) and §14.6 (the controller reset), plus making the
  already-landed stamp live.

No ruling needed; recorded because "§14.6" appearing in a slice-7 brief
after ruling 130 looks like the exact mistake ruling 130 corrected, and
an implementer who greps rulings will hit L3337 before L3480.

---

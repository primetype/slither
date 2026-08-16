# PLAN-7 — Slice 7: Mobility & contest

Base commit: `b649575` (clean tree). Planner: slice-7 planning agent.

**Status: COMPLETE, then updated for Round 30.**

> **⚠ Round 30 (rulings 168–180) landed while this plan was being
> written**, from an adversarial spec audit run blind to the planner
> (`AUDIT-7.md`). It rules ten of `QUESTIONS-7.md`'s eighteen items and
> **changes four things below**. `CONTRACT-7.md` is fully updated and is
> the authority; where this plan and the contract disagree, **the
> contract wins**. The four:
>
> 1. **Ruling 168** — the amplification budget now **disarms on a
>    return-routability proof**. §2.3 and §4/W3 predate it.
> 2. **Ruling 172** — the **2/2 fence split is ratified**, not the
>    uniform stamp §2.1 recommends. §2.1's analysis stands as analysis;
>    its recommendation is superseded.
> 3. **Ruling 180** — `FlakyWire::rebind`, and **in-flight datagrams are
>    abandoned, not carried**. §3.4's constraint 2 is reversed.
> 4. **Rulings 175/176/177/179** — the contested probe's rate bound, its
>    pending exits, its admission precondition and its closing carve-out.
>    §4/W5 predates all four.

Companion documents: `CONTRACT-7.md` (binding on every dispatched agent)
and `QUESTIONS-7.md` (18 items; ten now ruled).

Scope from `PLAN.md` line 287: §7.3 roaming + amplification budget · §7.5
keepalive + persistent keepalive + contested probe · §5.4 / §6.4 / §6.7–6.8
replacement + tie-break + restart · `notified()` + `Notification`.

Stories closed: **S3 (a/b/c), S4, S5, S11, S18, S19, S20, S27 (full)**.

---

## 0. Reading map (what the planner actually read)

Spec, by `Read` offset/limit only (working rule 1 — `SPEC.md` is 6 428
lines and was never opened whole):
§5.4 815–873 · §5.7 927–1018 · §6.3 1161–1309 · §6.4 1310–1457 · §6.7
1564–1662 · §6.8 1663–1712 · §6.9 1713–1841 · §7.2 1859–1892 · §7.3
1893–1946 · §7.4 1947–2044 · §7.5 2045–2455 · §13.6 3799–3821 · §14.6
3942–3977 · §15.4 4085–4121 · §16.2 (grepped) · §16.4 4815–5056 · §16.5
5057–5118 · §17.4 5510–5566 · §18.2 5726–5768 · App. B (grepped).

`STORIES.md`: S3 (76–103), S4 (104–119), S5 (120–139), S11 (211–235),
S18 (291–309), S19 (310–321), S20 (322–329), S27 (418–437).

`rulings.md` and the `src/` tree were surveyed by two read-only agents;
their findings are folded into §1 and §2 below.

---

## 0b. The eight stories, as acceptance criteria

Quoted tight, because the slice is done when these are paused-clock tests
that pass — not when the code compiles.

| Story | The pin | Anchor |
|---|---|---|
| **S3a** | `connect()` to a static that already has a live `Connection` returns `Err(ConnectError::AlreadyConnected)`; the first connection is **untouched**. | §5.4, §6.4, §16.1, §17.4 |
| **S3b** | A new handshake from an already-live static drops the old and installs the new — **only** with `replacement_basis == Some(t)` **and** a strictly newer candidate timestamp. `Some(t)` only where we were the **responder**. | idem |
| **S3c** | Where we dialled (basis `None`), the accept is refused `AcceptError::Stale` and the connection is marked **contested**. | idem |
| **S3 accepts** | The old connection's handle surfaces `ConnectionLost::Replaced`; in-flight stream data on it is lost; the new connection is independent with fresh stream state. | idem |
| **S4** | Both sides compare the same ordered pair of statics; the **lexicographically smaller static is the connection initiator for the life of the connection**, which also fixes stream-ID parity. Exactly one connection on each side and it is the *same* one. **Holds under both orderings, including when one side reaches the decision via the staged path and the other via the internal tie-break.** | §6.6, §6.7 (ruling 35) |
| **S5** | No-traffic connection emits nothing and dies at **install + `DEAD_TIMEOUT`** with `ConnectionLost::TimedOut`. **One exchange in either direction** ⇒ self-sustains indefinitely via the 10 s keepalive dance, **no opt-in**. | §7.4 install pin, §7.5 (rulings 39, 40, 42, 44) |
| **S5 opt-in** | `set_persistent_keepalive(Some(i)) -> Result<(), ConfigError>`; `Err` **below 1 s** and `Err` **at or above `DEAD_TIMEOUT`**; a rejected call leaves the current interval **unchanged rather than clamping**. | idem |
| **S11** | On refusal-by-`None`-basis: mark contested, ack-eliciting **PING** goes out, an ACK covering any counter **at or above the recorded probe floor** must arrive within `KEEPALIVE_TIMEOUT`. Live peer ⇒ survives, refusal standing. Dead peer ⇒ `ConnectionLost::TimedOut` and **the parked `Intro` becomes acceptable on the next attempt**. | §6.4, §7.5, §16.2 (36, 41, 43, 45, 46) |
| **S11 idempotence** | A second refusal while already contested creates **no second mark, sends no second PING, and does not re-arm the deadline**. | idem |
| **S11 visibility** | The marking is application-visible via §16.2's **notification stream**, not `ConnEvent`. Emitted at **probe transmission, not at marking** — "the two can separate under the amplification budget". Its **resolution is observable**. **No wire change.** | rulings 45 + 46 |
| **S18** | Authenticated, window-fresh packet from a new address re-homes; streams continue, no re-handshake, no data loss. App observes `ConnEvent::AddressMoved`; `remote_address()` reflects the new address. **The mover must send — the keepalive is what does it.** Positive obligation on the mover, not a transport probe. | §7.3, §7.2, §7.5 |
| **S19** | Our interface changes / NAT rebinds; the peer re-homes **on our next authenticated packet**. With the beacon enabled the binding is refreshed before the NAT drops it. | §7.3, §7.5 |
| **S20** | Peer restarts: where we were responder, the fresh initiation replaces the zombie immediately (S3b); where we dialled, refusal + contested probe resolves **within `DEAD_TIMEOUT`**, after which the reconnect succeeds. | §6.8, §7.5 |
| **S27** | `Connection::closed().await` resolves with the `ConnectionLost` reason **whenever the connection ends, for any reason, whether or not the application is inside a verb call**. Plus a **narrow per-connection notification stream**: `AddressMoved` and the contested marking **only** — deliberately **not** a mirror of `ConnEvent`. | §16.2, §16.4 (ruling 46) |

Two story-level ⚠ CHECKs are docs obligations this slice must discharge,
not merely note:

- **S3a's**: "an application that wants *reconnect now* must `close()`
  first, then `connect()`" — *"worth an explicit example in the docs,
  because 'call connect again' is the obvious wrong guess."*
- **S5's**: connecting ahead of need then sitting silent loses the
  connection at 25 s — *"it must be prominent in the crate docs, not
  buried."*

S19's ⚠ CHECK is a design note, not an obligation: *"an
application-visible story for 'we moved' versus 'they moved' may want
distinct handling; today both surface as address changes."* Slice 7 ships
the "today" behaviour; see QUESTIONS-7 Q13.

---

## 1. What already exists (code survey)

Surveyed at `b649575` by a read-only agent. **Slice 7 is unusually
well-prepared: eleven seams are already cut and uncalled.** The work is
mostly *wiring what exists*, not inventing.

### 1.1 The seams that exist and are dead

| Seam | Location | State at `b649575` |
|---|---|---|
| `TimerKind::{Contested, Keepalive, PersistentKeepalive}` | `src/core/connection/timers.rs:38,46,49` | declared, ordered, unit-tested; **armed by nothing**. `handle_timeout`'s arm is literally `{}` at `mod.rs:388–390` |
| `Recovery::on_roam(&mut self, now: Instant)` | `src/core/connection/recovery.rs:397` | written, **uncalled**; re-seeds `min_rtt` only |
| `NewReno::reset(&mut self, now: Instant)` | `src/core/connection/congestion.rs:81` | written, **uncalled**; sets `recovery_start = Some(now)` |
| `SentPacket.path_gen: u32` | `src/core/connection/recovery.rs:66` | written `0` at `mod.rs:1615`; read **only** by `debug_assert_eq!(packet.path_gen, 0, …)` at `recovery.rs:198` |
| `Liveness::last_send()` | `src/core/connection/session.rs:315` | exposed with the comment *"Read by slice 7's keepalive"* |
| `EstablishedSession.anchor: SocketAddr` | `src/core/mod.rs:191` | the peer address. Read at `mod.rs:1307`, `:1595`, `:1668`. **`Session::established()` returns `&`; there is no `&mut`** |
| `ConnCell.remote_address` | `src/shell/shared.rs:182` | shell mirror; written only at `ConnCell::new` and `driver.rs:644` |
| `ConnCell.notifications: NotificationSlots` | `src/shell/shared.rs:187–194`, `:584–589` | a `#[derive(Debug, Default)] pub(crate) struct NotificationSlots;` — *"A unit struct rather than an empty enum: it is a **place**, not a claim about which kinds exist."* |
| `slither::roam` trace target | `Cargo.toml` (§18.2's closed list) | ratified; **nothing emits on it** |
| CC-gate exemption slot for the probe | `mod.rs:1517–1527` | the `probe: bool` flag already threads `pump_inner` → `pump_packets`; the gate is `if ack_eliciting && !probe && !self.admits(size)` |
| `ConfigError::{KeepaliveTooShort, KeepaliveTooLong}` | `src/error.rs:293,296` | declared and pinned by `tests/spec_errors.rs`; **never constructed** |
| `ConnectionLost::Replaced` | `src/error.rs` | declared; **never constructed** |
| §6.4's proven-LIVE admission | `src/core/endpoint/staged.rs:617–621` | `Some((StaticState::Live, _)) => { discard_chain; Err(AcceptError::Stale) }` — the deliberate hole, documented at `endpoint/mod.rs:26–41` |
| `AMPLIFICATION_FACTOR` | `src/constants.rs:438` | defined, const-asserted at `:621`; **no budget counter anywhere** |

### 1.2 The three facts that shape the work most

**(a) The source address already reaches both cores; only the connection
core throws it away.** `driver.rs:267–270` takes `src` from
`wire.recv_from`, `driver.rs:783–809` passes it to
`endpoint.handle_datagram(now, src, …)` **and** to
`core.handle_datagram(now, src, …)`, and `core/connection/mod.rs:275–277`
opens with `let _ = src;`. **The plumbing for roaming is complete end to
end.** Slice 7 deletes one line and adds the gate behind it.

**(b) Routing is by index and has no address dimension at all.**
`endpoint/mod.rs:649` routes a Data packet on `indices.session(receiver_index)`
alone. `EstablishedSession.anchor` is the *only* durable per-connection
address in the core, and `StaticMap` holds `dialled: Option<SocketAddr>`
for pendings only — §17.4's *"the endpoint tracks no per-connection
address"* is literally true of the code. So **a roam is a single-field
mutation inside one connection**, invisible to the endpoint, and it
introduces **no new address-keyed map**. `intro_queue.rs`'s module doc
makes "no second address-keyed map appears" an explicit review criterion
(`:17–25`) — the design satisfies it by construction.

**Today a NAT rebind produces a working inbound path and a permanently
stale outbound one, invisible to every accessor except a wire tap.** That
is the exact bug S18/S19 close.

**(c) The keepalive's plumbing exists; the frame layer is untouched.**
`Frame::Ping` is fully wired (`frame.rs:70,96,117,156,712,791,825`,
`Packing::ping()` at `:948` with a `Stage::Ping` that is already the last
stage). §3.4's empty plaintext already short-circuits the frame layer at
`mod.rs:321–326` and `frame.rs:686–690`. **Slice 7 adds no frame type**,
and `constants.rs:668–685` asserts the table has exactly 14 named types —
a test that goes red only if someone adds one.

### 1.3 What has to be built

1. `Connection` gains an address field or a `&mut` route to
   `EstablishedSession.anchor`, plus `path_generation: u32`.
   `Session::established()` is `&`-only today — **a new accessor is
   required**, and it should be narrow (`roam_to(addr)`), not a blanket
   `established_mut()`.
2. The roam gate in `handle_datagram`, downstream of
   `session.open()`'s authenticate-then-window-check (`session.rs:559–561`
   is already the exact *"authenticated **and** window-fresh"* predicate
   §7.3 needs — it refreshes liveness at that point and nowhere else).
3. `ConnEvent::AddressMoved { from, to }` — the first new core event since
   slice 6. `mod.rs:20–23` states the policy the addition satisfies:
   *"the `ConnEvent` variants those sections define are **absent rather
   than stubbed**."*
4. `ConnEvent::Contested`, `ConnEvent::ContestCleared`.
5. The amplification budget: two `u64` counters on the connection, reset
   at each arming event, checked at the admission point.
6. Keepalive + persistent keepalive: arm the two declared timers, add
   `set_persistent_keepalive` at the shell, construct the two declared
   `ConfigError` variants.
7. The contested state and the `Contested` timer.
8. §6.4's LIVE branch at `staged.rs:617–621`, replacing the unconditional
   `Stale` — and `ConnectionLost::Replaced`'s first construction.
9. Shell: `Notification`, `notified()`, `NotificationSlots`'s real fields,
   and one new arm in `driver.rs:493–599`'s `publish` dispatch table.

### 1.4 Two incidental findings

- **Stale residue in `src/core/connection/mod.rs:87–90`**: two commented-out
  module declarations, `// mod tests_datagram;` and `// mod tests_message;`,
  whose files **do not exist on disk**. Slice 6's core-level unit-test files
  were never uncommented or never landed (its tests are in
  `tests/story_datagram.rs` and `tests/story_message.rs`). Rule 15 says the
  uncomment is the integrator's step; it did not happen. **Not slice 7's to
  fix, but the integrator should decide** — leaving them invites a slice-7
  agent to "helpfully" create the files, which is the slice-2a accident.
- **`tests/` has 11 files and `Cargo.toml` has 8 live `[[test]]` stanzas**
  (`Cargo.toml:171–217`), with **no commented-out ones**. The three
  unstanza'd files (`spec_constants`, `spec_errors`, `spec_packet`) need no
  `test-util`. Slice 7's new integration files each need a stanza with
  `required-features = ["test-util"]` — **the integrator's, per rule 15.**

## 2. Carried debts

Each of the three is answered here; each also produced a question, because
answering them exposed something the spec does not say.

### 2.1 Ruling 137 — `SentPacket` path generation and §13.6's four fences

**Status: the field exists and is held at 0. Slice 7 makes it live.**
Ruling 137 (`rulings.md` L3470–3485): *"a `u32` on `SentPacket`,
documented as **slice 7's**, asserted 0 in slice 5. One dead field against
a schema change in the densest remaining slice, and it pre-empts a defect
that would otherwise be written into four fences."*

**What each fence actually needs** — the brief's question, worked:

| # | Fence (§13.6 L3808–3811) | What gates it today | Can the recovery marker serve it? | Can the stamp? |
|---|---|---|---|---|
| 1 | no congestion event | §14.3's `sent_time ≤ recovery_start` | **Yes** — §14.3 L3862–3867 confirms it gates the event | Yes |
| 2 | no persistent-congestion walk | §14.4's walk, inside §13.2's loss detection | **No** — §14.3's test gates *"both the event and the growth"*, and the walk is neither. §14.4 L3886 states the obligation (*"Packets sent before a roam are excluded from the walk"*) and **names no mechanism** | Yes |
| 3 | no RTT sample | §13.1's newly-acked-`largest` rule | **No** — ruling 137's own argument: `recovery_start` is set by every ordinary congestion event, so reusing it *"would suppress RTT sampling after every normal loss episode — silently, and permanently on a lossy path"* | Yes |
| 4 | no `app_limited` growth | §14.3's same test | **Yes** — same sentence as fence 1 | Yes |

**Answer to the brief: the four cannot be served by the marker, and the
single `u32` slice 5 landed serves all four.** §14.6's amendment assigns
1 and 4 to the marker, 3 to the stamp, and **never assigns 2** — see
QUESTIONS-7 **Q3**, which also gives the argument that the split is worse
than the uniform stamp on a **paused clock**, where pre- and post-roam
sends share an `Instant` and `≤ recovery_start` then fences post-roam
packets. That needs a ruling before the implementer writes it.

**Implementation shape** (subject to Q3's ruling):

- `Connection` gains `path_generation: u32`, starting at 0, incremented
  on each committed roam (the same instant `ConnEvent::AddressMoved` is
  generated).
- `SentPacket.path` is stamped with the value current **at seal time**.
- Every one of the four fences reads `pkt.path != conn.path_generation`.
- `recovery_start` is still set to the roam instant (§14.6 L3951–3952,
  *"set to the roam instant — not cleared"*) — it does separate work,
  suppressing one legitimate new-path cut, and must not be dropped as a
  side-effect of moving the fences off it. See **Q4**.
- The sent map is **kept** across the roam (§13.6 L3804), `bytes_in_flight`
  stays consistent, loss detection and PTO *"continue undisturbed"*, and
  `pto_count` is **not** reset (nothing says to reset it, and ruling
  139(a) makes `pto_count`'s increment a property of the timer firing).
- The RTT estimator is *"suspect-but-kept"*, with `min_rtt` **re-seeded
  from the first post-roam sample** (§13.6 L3814–3815). Re-seeded, not
  cleared: the PTO floor may **rise**.
- cwnd resets to `INITIAL_WINDOW`, ssthresh to `u64::MAX` (§14.6 L3946).

**The consequence §13.6 states and the tests must pin** (L3816–3820):
*"immediately after a roam, `bytes_in_flight` may exceed the fresh initial
window; the admission gate then blocks new sends until old-path packets
are acknowledged or declared lost — a bounded stall of at most one
loss-detection/PTO cycle, kept probeable by the PTO exemption within
§7.3's budget."* Rule 9 applies: a test for this must **fill the window
before roaming**, or the degenerate implementation that resets nothing
passes.

### 2.2 Ruling 155 — the max-size-datagram delay bias vs. new control frames

**Answer to the brief's two questions:**

**(a) Does slice 7 worsen the bias measurably?** It depends entirely on an
unruled packing precedence — see **Q5**. Slice 7 adds two output kinds:

- **The keepalive** is the **empty plaintext** (§3.4 L671, §7.5 L2053) —
  a 16-byte tag-only ciphertext, a 30-byte datagram, which *"bypass[es]
  the frame layer"*. It carries **no frames at all**, so it exerts
  **zero** packing pressure and cannot displace a datagram. It is not a
  packet that a DATAGRAM could have ridden.
- **PING**, for the contested probe. PING already exists (§8.3 `0x01`,
  already used by §13.4's PTO probes, already in slice 5's tree), so
  **slice 7 introduces no new frame type and no wire change.** What is new
  is a *second reason* to owe a PING.

The bias worsens **only** if §8.5's *"then PING last"* wins over its
*"at most one extends-to-end frame … in final position"*, forcing the
`0x31` LEN form and pushing a max-size datagram to the next packet.
Ruling 155 established that the `0x30` extends-to-end form is *"what makes
the ratified maximum reachable"* — `MAX_DATAGRAM_PAYLOAD` 1169 does not
fit in `0x31` — so under that reading a probe-owing packet **cannot carry
a maximum-size datagram at all**. Under the other reading the probe rides
its own packet and the bias is untouched.

**(b) Does §8.5's packing order still hold?** Yes as an *order* — ACK,
control frames, STREAM/DATAGRAM fill, PING last — and slice 7 changes
nothing about it. What it does not resolve is the two-frames-in-final-
position collision, which was latent before slice 7 (a PTO probe could
already hit it) and becomes routine now. **Q5 asks for the precedence.**

### 2.3 The amplification budget vs. the contested probe (S11's separation)

**The separation is real and the plan keeps it real by construction**, not
by convention:

1. **The mark** records the probe floor and nothing else (`Contested::
   Pending { floor }` — Q1's shape). It emits **no** `ConnEvent`, arms
   **no** timer, and traces `slither::policy` only.
2. **The transmission** — the first instant §7.3's budget admits the PING
   — is the single event that (i) sends the PING, (ii) arms the
   `Contested` timer for `KEEPALIVE_TIMEOUT`, and (iii) generates
   `ConnEvent::Contested`. §15.4 L4093 pins all three to one instant:
   *"at the first instant §7.3's budget admits it, **which is also when
   the deadline arms and when `Contested` is emitted**"*.
3. **The verdict** transmits nothing (§15.4) and arrives as
   `Closed(ConnectionLost::TimedOut)`.

**Why the separation would otherwise collapse silently, and how the plan
stops it:** on a connection where the budget is never armed, marking and
transmission coincide, so an implementation that fires `Contested` at the
mark passes every test that does not roam first. §16.4 L4970–4972 states
that the pending state is *"reachable exactly when the connection has just
roamed to an unvalidated address"*, and App. B L6224–6225 instructs the
test to **roam first**. Three consequences for this plan:

- **CONTRACT-7 §3 states the reachability condition explicitly**, because
  neither §7.5 nor §6.4 mentions it and a test author reading only those
  will not find it (see Q2).
- **The pending-state tests are named as a required obligation** in §6
  below and assigned to a test author, not left to emerge.
- **Rule 9 check:** the separating assertion is *"`notified()` yields
  **nothing** while the mark is pending"* (App. B L6226). "Fires at
  transmission" alone is satisfied by the collapsed implementation; "emits
  nothing during the gap, then exactly one at transmission" is not.

There is a further budget/probe interaction the spec does not resolve —
whether the **roaming packet itself funds the budget it arms** — and it
bites hardest on exactly S18's mechanism, a keepalive-driven roam, which
is the smallest packet in the protocol. See **Q9**.

## 3. The fixture question — can `FlakyWire` express S19?

**Answered explicitly, as the brief requires. The answer is no, and the
extension is part of this slice with a named owner.**

Working rule 13 — *the fixture bounds the coverage* — applies with full
force: slice 7 is the mobility slice and **the fixture has no notion of a
local address changing.**

### 3.1 What the fixture models today

`FlakyWire.addr: SocketAddr` is a **private, non-`Cell` field**
(`src/testutil/mod.rs:578`). Its only reader is `local_addr()` (`:589`).
There is **no `rebind`, no `set_addr`, no interior mutability**.

- `Network::endpoint(addr)` (`:431–463`) **asserts the address is not
  already registered and panics otherwise**. There is no unregister and no
  move. `Inner.endpoints` is `BTreeMap<SocketAddr, EndpointState>` — one
  inbox per address, forever.
- `send_to` stamps the source unconditionally from `self.addr`, into both
  the tap (`Spied { src: self.addr, … }`, `:684–688`) and the delivery
  (`:708`).
- `recv_from` reads `net.endpoints.get(&self.addr)` (`:736`).
- `EndpointBuilder::wire(w: W)` takes the wire **by value**
  (`shell/endpoint.rs:345`). `SharedWire(Rc<FlakyWire>)` exists **only** so
  a test can change the `FlakyPolicy` mid-run, not the address.
- The per-wire RNG seed is derived from the **registration ordinal**
  (`:452–453`), and `FlakyWire`'s draw order is documented as *contract*
  (`:648–657`).

### 3.2 The one lever that exists, and its limit

```rust
// src/testutil/mod.rs:537
pub fn inject(&self, from: SocketAddr, to: SocketAddr, bytes: &[u8])
```

*"Deliver a datagram that appears to come from `from` — an address no
`FlakyWire` need own. The forgery fixture."* It bypasses `Tap`,
`Network::sends()` and every `FlakyPolicy` knob, and `deliver` drops the
datagram if `to` has no registered inbox (`:553`).

**So `inject` is one-way.** A test can make A's core see a packet from a
new source — but B cannot **receive** at that source, because there is no
inbox there. The exchange dies after one packet.

### 3.3 The verdict, story by story

| Story | Reachable at `b649575`? | Why |
|---|---|---|
| **S18** — *the peer* changes network, **core level** | **Yes**, with a small `testfix` addition. `Solo::deliver` (`testfix.rs:920`) hard-codes `a_addr()` as the source and `Pair::flush_a_to_b`/`flush_b_to_a` (`:561`, `:573`) hard-code both. A `deliver_from(now, src, frames)` is needed — a few lines. |
| **S18** — **integration level, round trip** | **No.** Tapping B's bytes and `inject`ing them from `addr_c()` gets A to roam, but A's replies then go to `addr_c()`, which has no inbox, and B never hears them. The connection dies at `DEAD_TIMEOUT` and the test proves nothing about *"streams continue with no data loss"* — which is S18's actual acceptance. |
| **S19** — *our* address changes / NAT rebind | **No, in every direction.** `FlakyWire.addr` is immutable, the wire was moved into the builder by value, `Network::endpoint` panics on re-registration, and `Wire` (`shell/wire.rs:61–66`) has **no notion of a local address at all** — neither method exposes or accepts one. |

**S19 is not expressible. It is a first-order acceptance criterion of this
slice.** Rule 13's lesson stands: no amount of test-writing against the
present fixture would find an S19 defect, so the fixture must move first.

### 3.4 The proposed extension — and why this shape

**Recommended: (b), a relocatable inbox, additive.** Add to `Network`:

```rust
impl Network {
    /// Move an endpoint's inbox from `from` to `to`, as an interface
    /// change or a NAT rebind does. The queued datagrams, the `Notify`
    /// and the endpoint's own RNG stream all travel with it: a rebind is
    /// a change of address, not a new endpoint.
    ///
    /// Panics if `from` is unregistered or `to` is already registered.
    pub fn rebind(&self, from: SocketAddr, to: SocketAddr);
}

impl FlakyWire {
    /// This wire's current local address. Changes under
    /// [`Network::rebind`].
    pub fn local_addr(&self) -> SocketAddr;   // exists; becomes non-constant
}
```

with `FlakyWire.addr` becoming `Cell<SocketAddr>`, and `Peer` gaining a
`rebind(&self, to: SocketAddr)` convenience plus `Peer.addr` becoming an
accessor rather than a `pub` field.

> **⚠ RULED — 180.** The verb is `FlakyWire::rebind(new_addr)`, not
> `Network::rebind(from, to)`. Constraint 1 below is **upheld and is the
> ruling's own emphasis** (a fresh `Notify` hangs the driver).
> **Constraint 2 is reversed: in-flight datagrams are ABANDONED, not
> carried** — the new address gets a fresh empty inbox. That is what a
> real interface change does, and it is what makes S18's positive
> obligation on the mover bite: under carry-across a peer could move,
> stay silent and still receive, so the story would pass **for the wrong
> reason** (working rule 9). Constraint 4's question does not arise —
> vacating the old key handles everything sent after the rebind for free,
> with no change to `deliver`.

**Four constraints the implementation must honour**, each derived from
something already documented in `testutil`:

1. **Carry the `Rc<Notify>` across the move.** `FlakyWire` holds its own
   clone at `FlakyWire.notify`; if `rebind` creates a fresh
   `EndpointState` with a new `Notify`, the receiver **deadlocks** — it
   waits on a handle nobody signals. This is the single most likely bug in
   the extension and it fails as a hang, not an assertion.
2. **Carry the queued `Reverse<Queued>` heap.** In-flight datagrams aimed
   at the old address are exactly the interesting ones (a rebind mid-flight
   is the realistic case).
3. **Do not disturb the RNG stream.** The per-wire seed comes from the
   registration ordinal, and draw order is contract. `rebind` must keep
   the same `ChaCha20Rng` and the same `sent: Cell<usize>` — the precedent
   is `set_policy`'s documented *"the send index is **not** reset"*
   (`:594–597`).
4. **Decide, and document, whether `partitioned` and `blocked` follow the
   address.** Both are keyed by `SocketAddr`. The planner's
   recommendation: **they do not follow** — a partition is a property of a
   *place* in the fabric, and a peer that rebinds out of a partition has
   done exactly what a real one does. State it either way; silence here is
   a trap.

**Why not (a), `Cell<SocketAddr>` alone with no inbox move:** it makes
`send_to` stamp the new source while `recv_from` still reads the old
inbox, which is a *half-rebind* no real socket performs and would let a
test pass against an implementation that only roams one direction.

**Why not "just use `inject`":** §3.2. It cannot carry the reply, and
S18's acceptance is *"streams continue with no data loss"*, which needs
both directions.

### 3.5 Owner and ordering — this is the slice's one hard dependency

`src/testutil/mod.rs` is a **shared path**: the implementer needs
`rebind` to exist for nothing, and the mobility test author needs it to
write M11 and the S18 round-trip at all. Rule 6 forbids handing one path
to two concurrent agents.

**Therefore the fixture extension is a *pre-slice* commit, made before any
worktree is cut**, and it belongs to the **integrator** (rule 15's logic:
a file whose contents both blind agents depend on is not either agent's).
Concretely:

1. Integrator lands `Network::rebind`, `FlakyWire`'s `Cell<SocketAddr>`,
   `Peer::rebind`, and `testfix::Solo::deliver_from` / `Pair::flush_from`.
2. Integrator lands `CONTRACT-7.md`.
3. **Both** are in the commit the worktrees are cut from, and the briefs
   name that commit (working rule 14 — slice 4a's test author lost ten
   minutes and made two semantic guesses because its binding contract was
   uncommitted at the cut).

The fixture extension needs its own tests (a rebind delivers to the new
address, the old address is dead, an in-flight datagram survives the move,
the `Notify` still wakes a parked receiver). Those are the integrator's
too, and they are the proof that the fixture is sound **before** two
agents build on it.

## 4. Work breakdown

Seven work items. Each names the seam it fills, so the implementer is
extending rather than inventing.

### W1 — the roam seam through recovery and congestion (§13.6, §14.6)

- `Connection.path_generation: u32`, 0 at construction, `+= 1` at each
  committed roam.
- `SentPacket.path_gen` is stamped from it at `mod.rs:1615` instead of the
  literal `0`; **delete the `debug_assert_eq!(…, 0, …)` at
  `recovery.rs:198`** — that assert is ruling 137's tripwire and its
  removal is the marker that the field went live.
- Wire the four fences (§2.1, subject to **Q3**).
- Call the two written-but-uncalled seams: `Recovery::on_roam(now)` and
  `NewReno::reset(now)`.

### W2 — roaming proper (§7.3, §7.2)

- Delete `let _ = src;` at `core/connection/mod.rs:276`.
- Gate: **authenticated **and** window-marked**. `session.rs:559–561` is
  already exactly that point — it is where liveness is refreshed and
  nowhere else — so the roam check belongs beside it, not before it.
- **Three negatives, each a separate guard**: unauthenticated (fails
  `open`), replayed (fails `check_and_mark`), and **handshake packets
  never roam a live session** (§7.3 L1898) — that last one is free,
  because handshake packets never reach the connection core, but a test
  must still pin it.
- **The closing/draining state does not roam** — `mod.rs:1289` already
  records this and `transmit_close` already uses the anchor.
- Emit `ConnEvent::AddressMoved { from, to }`; move the anchor; bump the
  path generation; trace `slither::roam` (the target is ratified and
  nothing emits on it today).

### W3 — the anti-amplification budget (§7.3, rulings 168–171, 173)

> **⚠ Rewritten by Round 30.** CONTRACT-7 §3.2 is the authority. The
> shape is now **four** fields, not two: `validated: bool`,
> `validation_floor: u64`, `budget_sent`, `budget_recv`. The budget
> **disarms** on an ACK covering `>= validation_floor` from that address
> (168); it is funded by authenticated **and window-fresh** bytes (169);
> it is **per session** (170); and a **pending contested probe outranks
> every other output** to an unvalidated address (171). Ruling 173 adds
> `validation_floor` and both counters to §13.6's roam-reset list.

- Two `u64`s on the connection: bytes sent to the current address, and
  authenticated bytes received from it.
- Armed (both reset) on a roam **and** at an accepted initiation's msg1
  anchor. **Not** armed on a `connect()` address (Q2).
- Checked at the admission point in `pump_packets`, **outside** the
  `!probe` exemption — §7.3 L1925–1927 is explicit that the budget binds
  *"**all** output … **explicitly including** the §14.5 and §13.4
  congestion-window exemptions"*.
- Only authenticated bytes replenish (§7.3 L1922–1924).

### W4 — keepalive and persistent keepalive (§7.5)

- Arm `TimerKind::Keepalive` from the passive rule: received since last
  send, and no send for `KEEPALIVE_TIMEOUT`. **`S` is marking sends only**
  — `Liveness::last_send()` already exists for this, and see **Q7**.
- Arm `TimerKind::PersistentKeepalive` from the configured interval,
  re-armed from every marking send, **not** reset by receives.
- Both send the **empty plaintext** via `seal` (marking), not a frame.
- `Connection::set_persistent_keepalive(Option<Duration>)` in the core and
  at the shell; construct `ConfigError::KeepaliveTooShort` /
  `KeepaliveTooLong` — declared at `error.rs:293,296` and never yet built.
- **A rejected call leaves the interval unchanged** (ruling 44). No clamp,
  no panic — the setter is reachable across bubble-ffi to iOS where an
  unwinding panic is undefined behaviour.
- **No new constant.** Ruling 63 declined a named ceiling: *"A named
  ceiling would be a second place `DEAD_TIMEOUT` is written down, and
  therefore a place it can drift."* Compare against `DEAD_TIMEOUT`
  directly — which is what `PERSISTENT_KEEPALIVE_MIN`'s doc comment at
  `constants.rs:425–428` already says.

### W5 — the contested probe (§7.5, §6.4, rulings 175–177, 179)

> **⚠ Extended by Round 30.** CONTRACT-7 §5.1 and §4.1 are the authority.
> Four additions: a covering ACK arriving while **`Pending`** cancels the
> probe and **emits nothing** (176); a roam while pending **keeps** the
> mark and its floor (176); the mark requires an **admitted candidate**,
> and re-home-walk exhaustion marks nothing (177); the closing/draining
> carve-out is enforced at **§6.4's admission**, not only in §7.5 (179).
> Ruling 175 supersedes ruling 43's rate bound — **no cooldown**, and
> every re-mark records a **fresh** floor.

- The three-state contested field (**Q1**).
- Marking records the floor from `Session::next_counter()`
  (`session.rs:394`, already public to the crate).
- Transmission: send the PING via the existing `Packing::ping()` path with
  the existing `probe: bool` gate exemption, arm `TimerKind::Contested`,
  emit `ConnEvent::Contested`.
- Clearing: **any ACK covering any counter ≥ floor** — not the probe's
  packet (ruling 41). Emit `ConnEvent::ContestCleared`.
- Verdict: `ConnectionLost::TimedOut`, nothing transmitted.
- **No-op on a closing or draining connection** (§7.5 L2446–2448).
- Trace all three moments under `slither::policy` (§18.2 L5730).

### W6 — §6.4's LIVE branch (§5.4, §6.4, §6.8)

- Replace `staged.rs:617–621`'s unconditional `Err(AcceptError::Stale)`.
- Basis `Some(t)` **and** candidate strictly `> t` ⇒ replacement: fire
  `ConnectionLost::Replaced` on the old connection (its **first
  construction** in the tree) and install the new one, **fresh** —
  §5.4 L820–823, and see **Q15**.
- Otherwise `Stale`, connection untouched, **and against a `None` basis,
  mark contested**.
- The guard record is **reverted** on a basis-refused `Stale`
  (§6.4 L1387–1406) — the tie-break-winner case is the one exception and
  is already implemented (`staged.rs:767`, `keep_winner_side_record`).
- Per **Q14**, verify how much of §6.4's PENDING branch already landed in
  slice 4 before writing anything here.

### W7 — the shell (§16.2, ruling 46)

- `NotificationSlots` gains its real fields — **one slot per kind**, never
  a queue (§16.2 L4415).
- `Notification` (public, `#[non_exhaustive]`), `notified()`.
- `AddressMoved` merge: **oldest unclaimed `from`, newest `to`**.
- One new arm in `driver.rs:493–599`'s `publish` table translating the
  three new `ConnEvent`s into slots + wakers. **A new `Wakers` field**
  (`notification_wakers`) on `ConnCell`, alongside the five that exist.
- `set_persistent_keepalive` on the handle.
- The two crate-doc obligations from §8.

---

## 5. File-path partition

**No path appears twice.** Rule 6 is absolute; rule 15 names the residue.

### 5.0 Pre-slice, the integrator's — landed **before** any worktree is cut

| Path | Why the integrator |
|---|---|
| `src/testutil/mod.rs` | §3's fixture extension. Both blind agents depend on it; neither can write it (rule 6), and it must exist at the cut (rule 14). |
| `src/core/connection/testfix.rs` | `deliver_from` / `flush_from`. Same argument. **Also**: `parse_frames`'s panicking fallback (`:244–249`) — slice 7 adds no frame type, so **no arm is needed**, but the integrator must confirm the empty plaintext (zero frames) yields `vec![]` rather than tripping anything. Twice now a slice has had to extend `Wire` before its tests could run. |
| `.slices/07-mobility/CONTRACT-7.md` | Binding on both; must be in the cut commit. |

### 5.1 The implementer — `IMPL`

| Path | Item |
|---|---|
| `src/core/connection/mod.rs` | W1, W2, W3, W5 wiring; the new `ConnEvent`s |
| `src/core/connection/session.rs` | the roam accessor, the roam gate beside the window check |
| `src/core/connection/timers.rs` | arming the three declared timers |
| `src/core/connection/recovery.rs` | W1's fences; `on_roam` call site; the tripwire deletion |
| `src/core/connection/congestion.rs` | W1's `reset` call site |
| `src/core/connection/ack.rs` | the probe-floor coverage predicate |
| `src/core/endpoint/staged.rs` | W6 |
| `src/core/endpoint/tables.rs` | W6's basis reads, if any |
| `src/core/mod.rs` | `EstablishedSession` / `Install` if they move |
| `src/shell/connection.rs` | `notified`, `set_persistent_keepalive` |
| `src/shell/shared.rs` | `NotificationSlots`' real fields, the new waker map |
| `src/shell/driver.rs` | the `publish` arm |
| `src/shell/mod.rs` | `Notification` re-export |
| `src/error.rs` | only if a variant is genuinely missing — **expect no change**, both `ConfigError` variants and `Replaced` already exist |
| `src/constants.rs` | **expect no change.** A diff here is a wire question (Q9's possible const-assert is the one exception, and only after a ruling) |
| `src/lib.rs` | the two crate-doc obligations |
| `src/core/connection/tests_roam.rs` *(new)* | the implementer's **own** unit tests — a distinct filename, never a story file |

The implementer **declares** `#[cfg(test)] mod tests_keepalive;` etc. only
for files it owns. It **creates nothing** under any path below.

### 5.2 Test author **T-K** — keepalive, beacon, liveness (§6.1, §6.5)

| Path | Contents |
|---|---|
| `tests/story_keepalive.rs` *(new)* | S5, S27's `closed()` half — K1–K8, S1, S2 |

### 5.3 Test author **T-M** — mobility, contest, replacement (§6.2–6.4)

| Path | Contents |
|---|---|
| `tests/story_mobility.rs` *(new)* | S18, S19, S11, S20, S3, S4 — M1–M11, C1–C8, R1–R9 |

**Why two authors and not three:** the contested probe and roaming are
the same subsystem — C6 and C8 *require* a roam to reach the pending
state (Q2) — so splitting them would hand two blind agents one behaviour.
Keepalive/liveness is genuinely separable and is the larger half by test
count.

**If a third author is dispatched**, the only clean cut is
`tests/story_replacement.rs` taking **R1–R9 alone** (S3, S4, S20's
replacement half), leaving T-M with S18/S19/S11. R7 and C1 both touch
"the parked `Intro` after the zombie dies", so the brief must assign that
overlap explicitly to one of them.

### 5.4 The integrator — post-slice

| Path | Why |
|---|---|
| `Cargo.toml` | the `[[test]]` stanzas for `story_keepalive` and `story_mobility`, each `required-features = ["test-util"]`. **Rule 15, verbatim**: cargo does not warn about a `[[test]]` whose file is missing, it **refuses to parse the manifest**, so an implementer landing live stanzas commits a tree on which *no gate can run at all*. |
| `tests/spec_constants.rs` | only if a constant moves (expect not) |
| `tests/spec_errors.rs` | only if an error variant moves (expect not) |
| `src/core/connection/mod.rs:87–90` | the stale commented-out `mod tests_datagram;` / `mod tests_message;` (§1.4) — decide and act |
| `.slices/07-mobility/` | `IMPLEMENTATION-7.md`, `TESTS-7-*.md` |

**Paths nobody touches this slice:** `src/packet/`, `src/core/endpoint/intro_queue.rs`,
`src/core/endpoint/guard.rs`, `src/core/endpoint/routing.rs`,
`src/core/connection/{recv,send,streams,flow,datagram,stream_id,close,frame}.rs`.
A diff in `frame.rs` or `src/packet/` means the wire moved and needs a
ruling.

## 6. Test obligations (App. B) mapped to owners

App. B is non-normative but every obligation below is quoted from it, and
each is assigned. **T-K** = keepalive/liveness author, **T-M** =
mobility/contest author. The IDs are this plan's, for cross-reference from
the briefs.

### 6.1 Keepalive, beacon and liveness — owner **T-K**

| ID | Obligation | App. B |
|---|---|---|
| K1 | **The dance is automatic once traffic has flowed.** Install a pair, exchange **one** application message **in one direction only**, drive nothing further, assert both sides alive *"well past install + `DEAD_TIMEOUT`"* — *"with no `set_persistent_keepalive` call anywhere in the test"*. | L6141–6146 |
| K2 | **The negative companion.** The same pair with **no** exchange dies at install + 25 s, and *"transmits **nothing** in the interim"*. | L6131–6135, L6146–6147 |
| K3 | **Responder-side half-open.** Accept a replayed initiation whose initiator never speaks again; assert reclamation, not an immortal session. | L6136–6140 |
| K4 | **Connect-ahead-of-use is reaped, and it is a *receive* rule.** (a) first message at *t* = 24 s ⇒ still dies at *t* = 25 s unless the answer lands inside that second; **drop that first message** ⇒ dies after ≈ one PTO *"with data still queued"*. (b) same shape + beacon ⇒ lives. (c) same shape + any completed exchange before 25 s ⇒ lives. | L6148–6160 |
| K5 | **The beacon sustains a mutually idle link.** `set_persistent_keepalive(Some(10 s))` on **one side only**, no application traffic; both live indefinitely. Drop **one** beacon ⇒ neither dies. Drop **two consecutive** ⇒ both die at 25 s. | L6161–6168 |
| K6 | **SECV5-8 — simultaneous bidirectional keepalive loss kills.** Drop **both** directions' keepalive in the same interval; assert **both** sides fire `TimedOut`. | L6124–6130 |
| K7 | **The interval band, both directions.** Accept 1 s (inclusive), 10 s, and everything in `[1 s, 25 s)`. Reject 25 s, 30 s, 999 ms, 1 ms, `Duration::ZERO`. `None` accepted at all times. Each rejection is `Err(ConfigError::…)`, **never a panic**, and the interval is **unchanged** after a rejected call. | L6115–6121, L6169–6181 |
| K8 | **The two named regressions.** A test pinning the old floor at `DEAD_TIMEOUT` is *"the regression this obligation exists to catch"*; a test asserting 1 s is **rejected** is *"the mirror regression against over-reading ruling 42"*. Write neither; assert against both. | L6178–6181 |

**Rule-9 note for T-K.** K1 and K5 are both "still alive at *t*" bounds
that a *never-reaps-anything* implementation satisfies for free. K2, K4a
and K6 are their separating companions and **must be in the same test
file**, so the pair cannot be split by a later edit. Similarly K7's accept
list without K7's reject list is satisfied by a setter that accepts
everything.

### 6.2 Roaming and the amplification budget — owner **T-M**

| ID | Obligation | App. B / spec |
|---|---|---|
| M1 | **Roam re-homes the session.** Authenticated, window-fresh Data from a new source moves the endpoint; streams continue, no re-handshake, no data loss; `ConnEvent::AddressMoved` and `remote_address()` reflect it. | §7.3 L1895–1902, S18 |
| M2 | **Nothing unauthenticated and no replayed packet ever moves it.** Both negatives, separately: a garbage datagram from a new source, and a **replayed** (window-stale but authenticated) packet from a new source. | §7.3 L1896–1897, §7.2 L1872–1874 |
| M3 | **Handshake packets never roam a live session.** | §7.3 L1898–1899 |
| M4 | **CC reset with the pre-roam flight fenced.** Old-path losses *"fire no congestion event and no persistent-congestion collapse; the flight still resolves for retransmission"*; sent map kept; RTT kept as a prior with `min_rtt` re-seeded. | L6102–6105 |
| M5 | **The post-roam stall.** Fill the window, roam, assert `bytes_in_flight` may exceed the fresh `INITIAL_WINDOW` and that the gate blocks new sends for at most one loss/PTO cycle — *"kept probeable by the PTO exemption within §7.3's budget"*. | §13.6 L3816–3820 |
| M6 | **The budget caps all output** — *"PTO probes, the contested-connection probe, pure ACKs, CLOSE, and keepalives included"* — at 3× authenticated bytes received. | L6234–6238 |
| M7 | **A genuine roam clears the budget within ≈ 1 RTT.** | L6238–6239 |
| M8 | **A silent address dies by liveness** *"having received at most 3× what it sent"*. | L6239–6240 |
| M9 | **Only authenticated bytes replenish.** Unauthenticated / undecryptable datagrams claiming the address MUST NOT fund the budget. | §7.3 L1922–1924 |
| M10 | **Notification retention, O(1).** *"Roam twice without claiming, then `notified()` once: assert the single `AddressMoved` carries the **oldest unclaimed `from`** and the **newest `to`**."* Plus: a notification generated before the death is still claimable after it; `notified()` returns `Err(ConnectionLost)` **only once the slots are drained**; dropping a `notified()` future claims nothing and the next call yields the same notification. | L6265–6272 |
| M11 | **S19 — our address changes / NAT rebind.** The peer re-homes on our next authenticated packet; with the beacon on, the binding is refreshed before the NAT drops it. **Requires the fixture extension of §3.** | S19, §7.3 |

### 6.3 The contested probe — owner **T-M**

| ID | Obligation | App. B |
|---|---|---|
| C1 | **The base shape.** Dialled connection (basis `None`) live; park an `Intro` for the same static; `accept()`. Assert (a) `AcceptError::Stale`, (b) *"a PING goes out on the live connection immediately"*, (c) ACK covering the floor inside `KEEPALIVE_TIMEOUT` ⇒ alive, refusal standing; silence ⇒ `TimedOut` at the deadline, *"after which the parked `Intro` is accepted normally"*. | L6182–6190 |
| C2 | **The ruling-41 high-water-mark regression.** Same shape but **drop the probe PING itself**; let the ordinary PTO retry go out and be acked inside the deadline; assert the connection **lives**. *"Assert the same with an ordinary application Data packet in place of the PTO retry."* | L6191–6200 |
| C3 | **The collapse.** While contested, deliver a **second** parked `Intro` and `accept()` again: also `Stale`, **no second PING**, and — *"the load-bearing assertion"* — the connection **dies at the original deadline, not one `KEEPALIVE_TIMEOUT` later.** *"Repeat with a refusal every second for the whole interval: the death instant must not move."* | L6201–6208 |
| C4 | **The security case.** Feed the zombie **withheld genuine Data** (harvested, window never advanced past it) every 5 s throughout; assert it **still dies** at the probe deadline. | L6209–6214 |
| C5 | **Accounting and admission.** The probe is sent **with the congestion window full**, and nonetheless appears in the sent-packet map and in `bytes_in_flight`. | L6215–6218 |
| C6 | **The pending case.** A probe held by §7.3's budget leaves the mark **pending with no deadline armed**; the deadline arms at the eventual transmission. | L6218–6220 |
| C7 | **No-op on a closing/draining connection.** A contested mark taken there does nothing. | L6221–6222 |
| C8 | **The notification, pinned to transmission.** *"roam to an unvalidated address so §7.3's budget holds the probe"*, then assert `notified()` yields **nothing** while pending, `Notification::Contested` **at the instant the probe goes out** (*"the same instant the deadline arms, pinned together"*), and `ContestCleared` when the covering ACK lands. **At most one of each per mark** across a refusal storm; the never-answered path emits **no third notification** — the death arrives on `closed()` as `TimedOut`. | L6223–6233 |

**Rule-9 notes for T-M.** C1(b) alone ("a PING goes out") is satisfied by
an implementation that PINGs on every refusal — **C3 is what separates
them**, and C3's separating assertion is the *unmoved death instant*, not
the absent second PING (an implementation could suppress the PING and
still re-arm). C8's separating assertion is *"yields nothing while
pending"*, not *"yields `Contested` at transmission"*.

### 6.4 Replacement, tie-break, restart — owner **T-M** (see Q14 on scope)

| ID | Obligation | App. B |
|---|---|---|
| R1 | **S3a.** `connect()` to a static with a live connection ⇒ `Err(ConnectError::AlreadyConnected)`; the first connection **untouched**. | L5995–5996 |
| R2 | **The replacement basis.** Dialled ⇒ `None`, proven-LIVE `accept()` ⇒ `Stale`, connection untouched (*"the captured-msg1 injection test, run against a connection established by `connect()`"*). Accepted ⇒ `Some(t)`; candidate ≤ `t` ⇒ `Stale`; strictly greater ⇒ **replaces**. | L5958–5965 |
| R3 | **`Replaced` fires exactly at the replacing `accept()`, never earlier.** A withheld or replayed initiation left unaccepted *"destroys nothing"*. | L5940–5943, L5991–5993 |
| R4 | **One session per connection.** The replacement carries **nothing** — fresh streams, credit, recovery, counters — and the old connection *"dies whole with `Replaced`"*. | L6098–6101 |
| R5 | **The dialled-only static (SECV5-5).** An arbitrarily old captured initiation surfaces as an `Intro` **repeatably** — assert **more than once from a single captured packet** — and the live connection is untouched every time. | L6015–6020 |
| R6 | **No-record-on-`Stale` (SECV5-6).** A basis-refused `accept()` leaves the guard **byte-identical**; a later genuine initiation with a timestamp between the two is still admitted. | L6010–6014 |
| R7 | **Restart end-to-end.** A restarted peer's reconnect replaces via `accept()`; its zombie counterpart dies at the replacement **or** at liveness; *"no state merge is representable"*. | L5947–5949 |
| R8 | **S4 convergence.** Two endpoints both driven `read_identity()` → `connect()` → `accept()` at each other converge on **one** shared session with complementary roles and agreeing parity. **Both static orderings.** *"assert data flows in both directions rather than merely that a connection object exists."* | L5978–5984 |
| R9 | **Stream-ID parity is fixed by the tie-break at establishment** and never changes. | L5987–5990 |

R8/R9 may already pass at `b649575` (Q14). They are still written, and a
pass with no implementation change is the correct result.

### 6.5 Shell surface — owner **T-K** (closed/latch) and **T-M** (notify)

| ID | Obligation | App. B |
|---|---|---|
| S1 | **`closed()` resolves on every death with no verb in flight.** Park on `closed()` and nothing else — no `read`, no `recv_message`, no send — and drive each row: `PeerClosed`, `TimedOut` (silence past 25 s, **and separately the contested verdict**), `Replaced`, `ProtocolViolation`, `LocallyClosed`. | L6253–6261 |
| S2 | **The latch.** A second `closed()` after death resolves immediately with the same value; a `closed()` first awaited **after** the death resolves too; several concurrent `closed()` futures all resolve. | L6261–6264 |

### 6.6 Obligations added by Round 30

These are **not** in App. B yet — they follow from rulings 168–180 and
must be written or the rulings ship untested.

| ID | Obligation | Ruling | Owner |
|---|---|---|---|
| **N1** | **The budget disarms.** Arm it (roam or msg1 anchor), then deliver an authenticated, window-fresh packet from that address carrying an ACK covering `>= validation_floor`; assert output is no longer capped. **The separating negative:** an ACK covering only counters *below* the floor does **not** validate. | 168 | T-M |
| **N2** | **An accepting endpoint can serve.** The regression ruling 168 exists for: accept a connection, have the peer request a bulk transfer and reply with ACKs only, and assert throughput is **not** pinned at 3× the ACK volume. A test that only checks "msg2 fits in the budget" passes the broken build. | 168 | T-M |
| **N3** | **A replayed packet funds nothing.** Deliver an authenticated but window-**stale** packet to an unvalidated address; assert `budget_recv` is unchanged and no additional output is admitted. | 169 | T-M |
| **N4** | **Per session, not per address.** Two connections to one peer address; assert each has its own budget and one does not fund the other. | 170 | T-M |
| **N5** | **Probe priority under a scarce budget.** Arrange a budget admitting less than is owed with both a pending probe and an owed ACK; assert the **probe** goes first. | 171 | T-M |
| **N6** | **The roam-reset list.** After a roam, assert **every** item in §13.6's list: budget counters and `validation_floor` reset, controller reset, `path_gen` bumped, **pending contested mark intact with its floor unchanged**, sent map kept, PTO undisturbed, RTT kept with `min_rtt` re-seeded, **replay window not reset**. | 173, 176 | T-M |
| **N7** | **The pending mark's ACK exit.** Roam so the budget holds the probe, mark contested, then let any post-mark seal be acked while still pending. Assert: **no PING is ever sent**, `notified()` yields **nothing at all** — neither `Contested` nor `ContestCleared` — and **no verdict deadline is armed**. This is the stray-probe regression. | 176 | T-M |
| **N8** | **Marks require admission.** Park mac1-valid rubbish that fails the walk; call `accept()`; assert `Stale` and **no mark, no PING, no notification**. Separately assert re-home-walk **exhaustion** returns `Stale` and marks nothing. | 177 | T-M |
| **N9** | **No mark on a closing/draining connection**, asserted at `accept()`. | 179 | T-M |
| **N10** | **Re-mark records a fresh floor.** Mark, clear by ACK, mark again; assert the second floor is **strictly greater** than the first and that a replay of the first probe's ACK does not clear the second mark. Do **not** assert ruling 43's superseded one-per-10 s rate. | 175 | T-M |
| **N11** | **The fixture itself.** `rebind` delivers to the new address; the old address is dead; **in-flight datagrams are abandoned**; a parked receiver is still woken (the `Rc<Notify>` survived). | 180 | integrator, pre-slice |
| **N12** | **Timer order, the two new relations.** Collide `Contested` with `Loss` at one instant and `AckDelay` with `Keepalive` at one instant; assert the ratified order. Both are first reachable in this slice. | 174 | T-K (AckDelay/Keepalive), T-M (Contested/Loss) |

**Rule-9 note on N1 and N2.** N1 alone is satisfied by an implementation
that disarms on *any* ACK; N1's separating negative (a below-floor ACK
does not validate) is what pins it. N2 alone is satisfied by an
implementation that never arms the budget at all; **N3 and N5 are its
separating companions** and must live in the same file.

---

## 7. Risks and sequencing

**R-1 — mostly discharged by Round 30.** Q1, Q3, Q6, Q9 and Q14 are
ruled. **Q5 and Q7 remain unruled** and each still decides something two
blind agents would decide differently *and consistently*. Both carry a
stated default in CONTRACT-7 §0; a ruling before dispatch is cheaper than
an integration disagreement, because rule 14 puts the contract in the cut
commit and a contract amended afterwards reaches nobody.

**R-1b — ruling 168 is flagged by the maintainer as the one most wanted
attacked.** It reverses a recorded declination and is queued for the
post-slice adversarial protocol review. **Slice 7 builds it as ruled**
(rule 5), but the implementer should keep `validated` / `validation_floor`
in a shape that survives being *removed* — a single guarded predicate, not
a condition threaded through six call sites.

**R-2 — S4 is already built (Q14 → ruling 178).** Ruling 178 closes
ruling 91's open predicate: PENDING means membership in the pending
tables. §6.4's PENDING branch is slice 4's. **Do not dispatch an
implementer against it.** S4's tests are still written and run; a pass
with no implementation change is the correct outcome, and App. B
L5978–5984's convergence shape (both orderings, data flowing both ways) is
what makes that pass mean something.

**R-3 — the fixture extension is on the critical path for S19.** §3's
extension must land **before** T-M is cut, or M11 cannot be written. It is
the only piece of this slice with a hard ordering constraint between
agents, and rule 6 forbids the two owning the same path — hence the
pre-slice commit in §5.

**R-4 — `Notification` is the first shell-side retained state.** Ruling
58's rule (*"an adapter never claims ahead of its consumer"*) and §10.6's
no-unbounded-queue rule both bear on it. The mitigation is in the spec:
**one slot per kind**, `AddressMoved` merging oldest-`from`/newest-`to`.
An implementer who reaches for a `VecDeque` has rebuilt the thing §10.6
forbids. **CONTRACT-7 §5 states the slot model as binding**, not as
guidance.

**R-5 — roaming touches the congestion controller, which slice 5 owns and
slice 6 did not move.** `congestion.rs` and `recovery.rs` are the two
files where a slice-7 edit can turn a slice-5 test red. Sequence the
implementer to do §13.6/§14.6 **first**, run the full slice-5 suite, and
only then build the keepalive timers — so a red is attributable.

**R-6 — the paused clock and the `≤ recovery_start` test (Q3).** If the
ruling keeps the marker for fences 1 and 4, expect paused-clock tests
written from §13.6's prose to fail against a conformant implementation.
Whichever way it goes, **CONTRACT-7 must state the fence predicate
literally**, in code, so the test author asserts the same predicate the
implementer writes.

**R-7 — `parse_frames` and the empty plaintext.** The keepalive carries no
frames. If `testfix::parse_frames` treats a zero-length payload as a
parse failure rather than an empty frame list, every keepalive test dies
in the fixture. Named in CONTRACT-7 §9; owner is the implementer, and it
is a `testfix.rs` change, which is a **shared** path — see §5.

**Sequencing for the implementer** (one agent, in this order):

1. `path_generation` + the four fences + the roam seam (§13.6, §14.6).
   Run the whole slice-5 suite. Commit.
2. Roaming proper (§7.3, §7.2's freshness gate) + `AddressMoved` +
   `remote_address()` + the `slither::roam` trace. Commit.
3. The amplification budget. Commit.
4. Keepalive + persistent keepalive + `ConfigError`. Commit.
5. The contested probe, the `Contested` timer, `Contested`/`ContestCleared`.
   Commit.
6. §6.4's LIVE branch + `Replaced` (and, per Q14, only what is not already
   there). Commit.
7. The shell: `notified()`, `Notification`, slot retention,
   `set_persistent_keepalive`. Commit.

Rule 10: commit before each step, not after all of them.

---

## 8. Gates

The slice ends on the **full table** in `CLAUDE.md`, not on `cargo test`.
Rule 7: every gate is pasted with its output in the integration report.

| Gate | Command |
|---|---|
| Compiles | `cargo build --all-features --all-targets` |
| Format | `cargo fmt --all --check` |
| Lints | `cargo clippy --all-features --all-targets -- -D warnings` |
| Docs | `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` and `--all-features` |
| Tests | `cargo test` **and** `cargo test --all-features` |
| Release tests | `cargo test --release --all-features` |
| Wire pins | golden-wire + size/constant tests under `cargo test` |
| MSRV | `cargo +1.96 check --all-features --all-targets` |
| Supply chain | `cargo deny check` |

**Wire-pin expectation for this slice: nothing moves.** Slice 7 adds no
frame type (PING is `0x01`, already present), no header field, and no
constant. Rulings 36, 41, 43, 45, 46 and 137 each say *"no wire change"* /
*"wire-free"* in terms. **A red golden-wire test in slice 7 is a bug in
slice 7, not an expectation to update** — `CLAUDE.md`'s standing rule,
and here it has an unusually strong prior behind it.

**Two docs obligations are gate-adjacent** (both from STORIES' ⚠ CHECKs,
both `cargo doc` visible):

- S3a: an explicit crate-doc example that *"reconnect now"* means
  `close()` **then** `connect()`, *"because 'call connect again' is the
  obvious wrong guess."*
- S5: connect-ahead-of-need + silence loses the connection at 25 s, *"it
  must be prominent in the crate docs, not buried."*

Owner: the implementer (they are rustdoc on the shell types).

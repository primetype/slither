# Adversarial security review — SPEC-v2-DRAFT.md (phase 1)

Reviewer lens: **security only**. Review object:
`scratchpad/SPEC-v2-DRAFT.md` (DRAFT 2026/08/13). Conformance input:
`scratchpad/design-choices.md`. Evidence: `scratchpad/research-wireguard.md`,
`scratchpad/research-hiss-api.md`, `/Users/nicolasdiprima/work/primetype/slither/SPEC.md`
(v1, RATIFIED), and the v1 implementation under
`/Users/nicolasdiprima/work/primetype/slither/src/`.

Line numbers are 1-based into `SPEC-v2-DRAFT.md` as read.

Threat model assumed throughout unless a finding says otherwise:

- **A1.** The responder's static public key is public data (v1 §4: mac1's key is
  "not a secret authenticator"), so **anyone** can mint mac1-valid packets of
  any type. Passing stage 0 costs an attacker one keyed BLAKE2b.
- **A2.** Peer static public keys are public data (v1 §5 step 4 says so
  explicitly: "statics are public data").
- **A3.** Three attacker classes: *blind* (can send, cannot forge source
  addresses, cannot observe); *spoofing* (can forge source addresses, cannot
  observe); *passive on-path* (can observe, no drop capability); *active
  on-path* (observe + drop). Findings name which class is needed.
- **A4.** The application is honest but is expected to actually implement a
  policy — i.e. it drains `accept()` and calls `read_identity()`/
  `authenticate()` to decide. An application that never probes can never accept
  anyone and is not a useful baseline.

---

## BLOCKER findings

### 1. BLOCKER — the receive-backpressure shed rule lets a **replayed** packet roam the connection endpoint

**Where.** §8 *Receive backpressure*, lines 681–691.

**What the text says.**

> When it is full, an inbound packet is handled as: decrypt → replay **check**
> (not mark) → if fresh: update liveness and roaming (authenticity is
> established) → then, **only if the buffer has room**: replay **mark**, frame
> processing, ACK scheduling, delivery. A shed packet is never replay-marked and
> never ACKed […] This requires splitting the window's admit into *check* and
> *mark* — a pure-core change.

**The attack.** v1 §6 *Roaming* is carried into v2 by §2's table as "in full"
(only *observability* is amended by §8). v1 §6 states: "An **authenticated,
fresh** (non-replayed) Data packet whose source differs from the session's
current endpoint moves the endpoint to the new source. **Nothing
unauthenticated, and no replayed packet, ever moves it.**" The v1 implementation
enforces this atomically — `ReplayWindow::admit()` checks *and* marks in one
call (`src/session.rs:72–104`), and roaming happens only after `admit` returns
`true` (`src/session.rs:290–304`).

The shed rule breaks that atomicity. A packet with counter `c` that arrives when
the buffer is full is authenticated, roams/refreshes liveness, and is **left
unmarked**. `hiss`'s `DatagramRecv::decrypt_at` will open the same counter any
number of times (research-hiss-api §4.1: "`decrypt_at` will open the same
counter any number of times"). So:

1. Passive on-path attacker records Data packets from a busy connection.
   (Shedding is common and is *induced* by the legitimate peer sending faster
   than the application drains — no attacker capability needed to cause it.)
2. Attacker replays a recorded packet from a spoofed source `X`.
3. Our side: `decrypt_at` succeeds; replay **check** says *fresh* (counter `c`
   was never marked); we **update roaming → the session endpoint moves to `X`**
   and refresh `last_recv`.
4. All subsequent traffic for that connection is sent to `X`. The peer stops
   receiving. Both ends die at `DEAD_TIMEOUT` (15 s), and the attacker can
   re-arm the hijack with the next recorded shed packet.

Note this promotes a *passive* observer (who cannot drop and is not on the
forwarding path) into a **traffic redirector**. The attacker does not need to
know which counters were shed — replaying a batch of recorded packets and
letting the unmarked ones land is sufficient. Seq-level dedup does not help:
the roam happens before frame processing.

Secondary consequence of the same clause: `last_recv` can be advanced by a
replay, so an attacker can hold a dead session alive past `DEAD_TIMEOUT`.

**Fix.** Restore the v1 invariant: **mark the replay window on every
authenticated, fresh packet, shed or not.** Solve the ACK problem separately
rather than by leaving the window unmarked — e.g. keep a second 128-bit
`shed_mask` alongside the window and subtract it when building ACK ranges from
the snapshot (`src/session.rs:110–118`), so a shed counter is marked (replay
protection holds) but never acknowledged (the "sender would clear it: permanent
loss" hazard the clause correctly identifies is still avoided). This is still "a
pure-core change" and costs 16 bytes per connection. Whatever is chosen, the
draft must state explicitly that **liveness and roaming are driven only by a
packet that is both authenticated and window-marked**, and §2's table must stop
claiming v1 §6's roaming rule survives "in full" if it does not.

---

### 2. BLOCKER — the stage-0 queue's overflow policy converts a trivial flood into indefinite total denial of inbound accepts; the honesty clause is materially wrong about what the exposure is

**Where.** §4, lines 183–186 (constants), 194–206 (dedup key + overflow),
230–235 (*Honesty clause — the spoofed-source exposure*).

**What the text says.**

> `INTRO_QUEUE_CAP` | **1024** parked entries, endpoint-wide
> `INTRO_TTL` | **90 s** after the entry's last refresh
> **Dedup key: the full source `SocketAddr` alone; replace-with-newest** […]
> **Overflow: the incoming packet is silently dropped** — the queue is
> untouched; the initiator retransmits in ~5 s.
> **Honesty clause** […] a source-spoofing attacker can mint mac1-valid
> initiations from arbitrary addresses and occupy distinct queue slots up to the
> cap. The slots cost ~220 B each and **0 DH**; an application that never probes
> them spends nothing. This is WireGuard-equivalent exposure […]

**The attack.** Three compounding errors:

1. **No spoofing is required.** The dedup key is the *full* `SocketAddr`,
   including the port. A single attacker host with one real IP fills all 1024
   slots using 1024 distinct **source ports**. No forged source addresses, no
   on-path position, no guessing — only the responder's public static (needed
   for mac1, which is public per A1) and its address. The clause's
   "source-spoofing attacker" framing understates the required capability to
   nothing.
2. **The overflow policy makes the occupation sticky.** Because overflow drops
   the *incoming* packet and leaves the queue untouched, once the cap is reached
   **every genuine initiation from every peer is dropped**, endpoint-wide, and
   the genuine initiator's ~5 s retransmit hits the same wall until it gives up
   at 90 s. The queue is an admission *ration*, not a work queue; drop-incoming
   is the wrong policy for a ration.
3. **The sustaining cost is ~11 packets/second.** `INTRO_TTL` runs from the
   entry's *last refresh*, so holding 1024 slots forever costs 1024/90 ≈ **11.4
   pps ≈ 22 kbit/s** and one keyed BLAKE2b per packet. Even against an
   application that drains promptly, the attacker only has to out-rate the
   drain, and an application that *probes* (A4) drains at ~1 DH per entry
   (≈30–60 µs software P-256), i.e. ≤ ~20 k/s while burning 100% CPU on
   attacker-chosen DHs.

**The "WireGuard-equivalent" claim is wrong.** WireGuard's 4096 ring
(research-wireguard §2) is a *transient work queue* drained by kernel workers in
microseconds; its depth is a load signal (≥ 1/8 full ⇒ under-load ⇒ mandatory
mac2 + per-IP token bucket). slither's 1024 slots are held **for up to 90 s
pending an application decision**, with no under-load trigger and no per-source
cap. These are not the same exposure, and the deferral to cookies/mac2 (§11)
does not close it: WireGuard's *other* two defences (queue drained at line rate,
20 pps/burst-5 per-source-IP token bucket) are what actually bound this, and
neither is present.

**Fix.** Pick at least two of:

- (a) **Change the overflow policy to evict-oldest** (or evict-LRU) rather than
  drop-incoming, so a genuine initiation always obtains a slot and the attacker
  must win a race per packet rather than hold a permanent reservation.
- (b) **Add a per-source-IP slot cap** (e.g. ≤ 4 slots per source IP / per /64),
  which is the cheap 0-DH analogue of WireGuard's per-IP token bucket and kills
  the single-host-many-ports variant outright.
- (c) **Shorten the parked TTL** — 90 s is the *initiator's* give-up horizon, not
  the responder's obligation. An entry is superseded every ~5.3 s in the normal
  case; a 15–20 s parked TTL is ample and cuts the hold cost by 5×.

And **rewrite the honesty clause** to say plainly: (i) no spoofing capability is
required; (ii) filling the cap denies **all** new inbound accepts endpoint-wide,
not just memory; (iii) the sustaining rate; (iv) that this is *not*
WireGuard-equivalent, because WireGuard's queue is drained at line rate and is
additionally protected by the under-load cookie gate and a per-IP rate limiter.

---

### 3. BLOCKER — supersession keyed on source address alone lets any unauthenticated packet reset (and poison) a victim's in-progress staged accept

**Where.** §4, lines 194–204 (dedup key), 210–215 (*Supersession*).

**What the text says.**

> If a replacement arrives while the application is *between* stages (a
> mid-state exists), the mid-state — cryptographically bound to superseded bytes
> — is discarded, the fresh initiation re-enters at stage 0, and the next verb
> on the old chain returns `Superseded`.

**The attack.** The dedup key is the source `SocketAddr` alone, and stage 0
admits anything that is length-correct, correctly typed/versioned, and
mac1-valid — all of which an attacker can produce for free (A1). The msg1 body
need not be well-formed at all.

Peer `P` at address `A` initiates. The application pops the `Intro`, calls
`read_identity()` (1 DH), inspects the claimed static, and starts its policy
decision (a UI prompt, a directory lookup, an audit-log write — anything
non-instantaneous). An attacker who knows or can spoof `A`:

1. sends one mac1-valid garbage msg1 with source `A`;
2. the victim's `Claimed`/`Proven` mid-state — **already paid for with 1 or 2
   DH** — is discarded, and the parked bytes are replaced with the attacker's
   garbage;
3. the next verb on the app's chain returns `Superseded`;
4. the application restarts at stage 0 on **the attacker's bytes**: a probe now
   returns `Malformed` (or an attacker-chosen claimed static), having cost us
   another `es`.

Repeat at 1 packet per few hundred ms and **peer `P` can never be accepted**, at
0 DH to the attacker and 1–2 DH per packet to us. The application's own
retransmit-driven supersession (~5.3 s) already makes any accept decision slower
than ~5 s impossible; the attacker simply removes the 5 s grace.

This is a **regression against v1**. v1's responder processed each msg1
synchronously against the allow-list; a spoofed garbage msg1 died at the tail tag
and could not disturb a concurrent genuine handshake, because there was no
per-source parking slot for it to clobber.

**Assumption.** That "mid-state" in the supersession clause covers `Proven` as
well as the post-`read_identity` state. If it does not, the clause needs to say
so — but the same attack then simply targets the `Claimed` stage.

**Fix.** Decouple an in-progress chain from the stage-0 slot: **once
`read_identity()` consumes an entry, the chain owns its own bytes and its own
`IntroId`, and a later initiation from the same address parks as a *new* stage-0
entry with a new `IntroId`** (surfacing as a new `Intro`, per finding 9). The
existing bound still holds — mid-states remain capped by `INTRO_QUEUE_CAP`
because each consumes a slot until dropped — and the application decides which
chain to finish. `Superseded` then means only "a newer initiation exists", never
"your work was destroyed by an unauthenticated packet". If the maintainer keeps
the clobber semantics, the draft must carry an honesty clause naming this as a
0-DH, off-path, per-peer accept-denial primitive.

---

### 4. BLOCKER — simultaneous open does not converge: both sides cancel their own pending and install two *different* sessions

**Where.** §5 *Simultaneous open*, lines 315–326 (and design-choices H2).

**What the text says.**

> If the matched connection has an in-flight outbound initiation when the
> continuation installs a session, the endpoint **cancels that pending** (drops
> it and its index; no give-up, no error) — the simultaneous open resolves to
> whichever handshake completes first.

**The attack (and the plain bug).** "Whichever handshake completes first" is
evaluated *locally* on each side, and on each side the **inbound** handshake
always completes first. Let A and B dial each other within one RTT:

| t | A | B |
|---|---|---|
| 0 | sends msg1_A | sends msg1_B |
| RTT/2 | receives msg1_B → continuation → installs **S_A** (A responder), sends msg2_A, **cancels A's pending** | receives msg1_A → installs **S_B** (B responder), sends msg2_B, **cancels B's pending** |
| RTT | receives msg2_B → no pending index → **dropped** | receives msg2_A → no pending index → **dropped** |

A now holds S_A = keys from {msg1_B, msg2_A}; B holds S_B = keys from {msg1_A,
msg2_B}. **These are different key sets.** A's send key is not B's receive key.
Neither side can open the other's Data. Under the instant swap cut (§8, G2)
there is no previous keypair to fall back on. Both connections are mutually dark
until `DEAD_TIMEOUT` (15 s), at which point the application receives
`ConnectionLost::TimedOut` — after `Connecting` already resolved `Ok` on the
first `Install { initial: true }` (§5, lines 321–324).

The window is one full RTT wide, not measure-zero, and mutual dial is the
natural pattern for the stated target ("the family-devices set", v1 §5) where
both ends autoconnect. Under the one-connection-per-static rule (§3) there is no
second connection to carry the surviving session.

v1 was *less* broken here: v1 kept the pending alive (`src/endpoint.rs:762–780`
takes the attempt but the pending survives a failed completion), so a lost or
late msg2 on one side broke the symmetry and the sides converged; the reported
v1 defect (hazard 5.8) was a cosmetic spurious `Failed`. **H2 trades a cosmetic
event bug for a functional mutual blackout.**

**Fix.** Make the cancellation decision **deterministic and identical on both
sides** — a tie-break, not a race. Concretely: on receiving an inbound
initiation for a connection with an in-flight outbound pending, compare the
inbound initiation's decrypted timestamp against our own pending's initiation
timestamp (or, if a timestamp tie must be handled, compare the two static keys
lexicographically):

- if **our** initiation wins the tie-break: **silently drop the inbound msg1**
  (no `ss` beyond the already-paid authentication, no msg2, no guard record, no
  install) and let our own handshake complete;
- if the **inbound** wins: cancel our pending and run the continuation as drafted.

Both sides evaluate the same two values and reach opposite, complementary
conclusions, so exactly one session is built and both sides hold it. Then delete
the sentence "resolves to whichever handshake completes first" — it is false
under any rule that cancels locally.

---

## MAJOR findings

### 5. MAJOR — the eager-path demote-then-probe sequence costs **3 DH per attacker packet**, breaking both §4's cumulative cost table and §1's own reason for killing the fallback

**Where.** §5 routing rule step 3, lines 264–267; §4 typestate table, lines
132–137; §5 DoS table, lines 354–362; §1, lines 28–31.

**What the text says.**

> claimed ∉ known statics → discard the mid-state and **demote** the raw packet
> to the stage-0 queue under the normal §4 rules

and §4's table asserts *cumulative* responder cost `Intro` 0 / `Claimed` 1 /
`Proven` 2 / `Connection` 4 DH, and §5's table caps any attacker packet at 2 DH.

**The gap.** Trace one packet from a hint-set source claiming an unknown static:

1. eager path runs the split intro read → **`es` (1 DH)**;
2. claimed ∉ known statics → **the paid mid-state is discarded** and the raw
   bytes are re-parked;
3. the application probes the demoted `Intro` → `read_identity()` re-runs the
   split intro read from scratch → **`es` again (1 DH)**;
4. the application (which has now been handed an attacker-chosen static) calls
   `authenticate()` → **`ss` (1 DH)** → dies at the tail tag.

Total: **`es`, `es`, `ss` = 3 DH for one attacker packet**, and the *cumulative*
cost at `Proven` on a demoted chain is 3, not 2. This is precisely the
`es, es, ss` cost shape §1 rejects as disqualifying for the hiss fallback ("the
fallback re-read would break the ratified DH-cost pins — an accepted msg1 read
would cost three DHs, not two"), reintroduced through the demotion path. The
appendix-B obligation "the §4 cost table driven end-to-end" would fail on any
demoted entry.

**Fix.** Either (a) **carry the paid mid-state through the demotion** — park the
demoted entry with its mid-state attached, so `read_identity()` returns the
already-known claimed static at 0 additional DH. The bound is clean: demoted
entries are keyed by hint-set addresses under replace-with-newest, so their count
is ≤ |hint set| = the number of connections and pendings; or (b) amend §4's table
to state the demoted chain's costs (1/1/2/4 marginal, 1/2/3/5 cumulative) and
amend §5's table to a 3 DH ceiling — and then reconcile that with §1's rationale.
(a) is strictly better and preserves the ratified pins.

---

### 6. MAJOR — "off-path attackers do strictly better against slither v2 (0 DH)" is only true for an application that can never accept anyone

**Where.** §5 *DoS accounting*, lines 364–368; rows 2–3 of the table, lines
357–358.

**What the text says.**

> Off-path attackers do strictly better against slither v2 (0 DH) than against
> WireGuard.

**The gap.** The 0-DH row is conditioned on "the application never probes or
drops the `Intro`". An application that never probes cannot make a policy
decision and therefore never accepts anyone; it is not a system, it is a
sinkhole. Any application that implements the staged accept as designed calls
`read_identity()` on each parked `Intro` — so the *realised* cost of a
mac1-valid off-path packet is **1 DH, i.e. exactly WireGuard's baseline**, plus a
queue slot held for up to 90 s that WireGuard does not have (finding 2), plus —
if the application also calls `authenticate()`, which it must to learn the
*proven* identity — **2 DH**.

The honest statement is: *an attacker with no source-spoofing capability spends
nothing of ours in the endpoint core, and shifts the entire cost to an
application-visible, application-scheduled decision; a functioning application
pays 1–2 DH per attacker packet and one parked slot.* Calling that "strictly
better than WireGuard" without the slot cost and without the probe cost is a
wrong claim in the section whose whole purpose is honest DoS accounting.

Related, in the same paragraph: "Spoofing into the hint set requires knowing an
established connection's current 4-tuple — an on-path observer or a lucky guess."
The entropy is much lower than "a 4-tuple" implies: the hint check is on **`src`
only** (an `(IP, port)` pair — the peer's own socket address), not on a 4-tuple,
and the hint set *also* contains "the dialled addresses of all in-flight
outbound initiations", which for a bootstrap/relay/rendezvous peer is public
configuration. Against a peer on a well-known port the guess is ~0 bits.

**Fix.** Restate the conclusion in terms of *capability* rather than
*directionality*: "an attacker who cannot forge source addresses pays us 0 DH in
the endpoint core, and 1 DH (probe) or 2 DH (probe + authenticate) per packet
against any application that implements a policy, plus one parked slot per
distinct source address." Replace "an established connection's current 4-tuple"
with "the source `(IP, port)` of any established connection **or any address we
have dialled**", and state the entropy honestly.

---

### 7. MAJOR — the DoS table is missing the replayed-genuine-msg1 row, and its "capped in rate by the pacing gate" row is wrong

**Where.** §5 *DoS accounting*, lines 354–362; §5 pacing, lines 303–313.

**What the text says.**

| forged claim of a known static […] | 2 DH (`es` + `ss`), dies at the tail tag |
| genuine replacement initiation from a key-holding peer | 4 DH, capped in rate by the pacing gate |

and: "It gates *acceptance*, never cost — exactly as in the kernel."

**The gap — two distinct wrong/absent rows.**

1. **Replayed genuine msg1, no row at all.** A passive on-path observer records
   one genuine msg1 from an established peer and replays it at line rate from
   that peer's address. Each replay: `src ∈` hint set → eager `es` (1 DH) →
   claimed static is known → continuation → `complete()` **succeeds** (the bytes
   are genuine, so the tail tag verifies) → `ss` paid (1 DH) → the *timestamp
   guard* rejects it. **2 DH per replayed packet, unbounded rate, from a single
   recorded packet, and the pacing gate does not cap it** because pacing is
   checked alongside the guard, i.e. after both DHs. This is the cheapest
   sustained 2-DH primitive in the design and it is not in the table.
2. **The "4 DH capped" row conflates cost with acceptance.** Per the clause's own
   (correct) statement that pacing gates acceptance and never cost, a
   key-holding peer flooding replacement initiations costs us **2 DH per packet
   uncapped** and 4 DH only for the ≤50/s that are accepted. The row as written
   reads as though the whole 4 DH is rate-capped.

**Fix.** Add the missing row —

| replayed genuine msg1 from a hint-set source (recorded once, replayed at line rate) | **2 DH** (`es` + `ss`), dies at the timestamp guard; **not** capped by the pacing gate |

— and split the last row into "2 DH per initiation (uncapped); +2 DH only for the
≤ `INITIATIONS_PER_SECOND` accepted". Then state the resulting **ceiling
explicitly**: with finding 5 fixed, the maximum cost of any single attacker
packet is 2 DH, and only 1 of those is reachable without either the attacker
holding a hint-set address or the application choosing to spend.

---

### 8. MAJOR — the guard-eviction honesty clause omits that eviction is attacker-triggerable at will, and that LRU order evicts exactly the wrong entries

**Where.** §7 *Bounding the timestamp guard*, lines 584–599.

**What the text says.**

> All other entries (orphans: authenticated-then-dropped intros, dead
> connections) live in a **bounded LRU** […] `TS_GUARD_ORPHAN_CAP` | **1024**
> […] Evicting an orphan re-admits a replay of that static's last initiation
> […] This is exactly the exposure WireGuard accepts on responder restart

**The gap.** The clause reads as though eviction is an incidental capacity event.
It is a primitive the attacker controls:

- Orphan entries are written by `authenticate()` (post-`ss`), so they require a
  key — but **the attacker supplies the keys**. Generating 1024 fresh keypairs is
  free. The attacker sends 1024 genuine initiations from 1024 source ports (see
  finding 2 — no spoofing needed); the application authenticates them (which is
  what an application does to learn a proven identity), drops them, and **1024
  fresh orphan entries evict every pre-existing orphan.** Incidental cost to us:
  2048 DH.
- **LRU order is adversarially optimal.** The attacker's entries are all
  maximally recent, so eviction targets the *least recently used* orphans — i.e.
  precisely the long-idle legitimate peers whose replay protection has the most
  historical depth and whose old msg1s an attacker is most likely to have
  recorded.
- The consequence goes one step beyond "surface as a fresh `Intro`": if the
  application accepts the replayed initiation, it observes a **`Connection` to a
  peer that never connected**, at an address the replayer chose. It dies at 15 s
  liveness and no key material leaks, but any policy side-effect keyed on "peer
  L is online" fires on a forgery. (Self-healing is real but conditional: L's
  genuine initiation arrives from an address that is *not* in the hint set —
  the hint holds the replayer's address — so it takes the parked path and depends
  on the application draining the queue, which §5 elsewhere promises rekey
  processing never does.)

The WireGuard analogy is sound as far as it goes but is being used to cover a
different fact: WireGuard's version of this is a **responder restart**, a rare
operator event; here it is an unauthenticated party's choice, on demand.

**Fix.** Keep the mechanism; make the clause complete. Add: (i) the attacker can
force eviction with ~1024 authenticate-then-drop chains using self-generated
statics; (ii) LRU evicts the longest-idle legitimate peers first; (iii) the
observable consequence is a spurious `Connection`/`Intro` attributed to a real
peer at an attacker-chosen address. Consider a cheap mitigation that costs
nothing structurally: **do not create an orphan entry for a static that was
authenticated and then rejected without ever being accepted** — or age orphans
out on a timer (e.g. `INTRO_TTL`) so the *volume* of attacker entries does not
translate into eviction of durable ones; a time-bounded orphan tier is a strictly
better fit than an LRU here.

---

### 9. MAJOR — `Superseded` is unrepresentable in `AuthError`, and `AcceptError` is never enumerated

**Where.** §4 *Errors*, lines 172–179; §4 supersession, line 215; §6 core
surface, lines 434–437.

**What the text says.**

> `AuthError::{Replay, HandshakeFailed, Expired}`

but line 215 makes it normative that "the next verb on the old chain returns
`Superseded`" — and the next verb on a `Claimed` chain is `authenticate()`
(returns `AuthError`), and on a `Proven` chain is `accept()` (returns
`AcceptError`, whose variants appear nowhere in the draft).

**Why it matters for security.** These are the error paths an application uses to
distinguish "this peer failed authentication" (`HandshakeFailed` — a security
event worth logging/alerting) from "your handle went stale" (`Superseded` — a
routine liveness event, and, per finding 3, an attacker-inducible one). With no
`Superseded` variant on `AuthError`, an implementation must either invent one or
collapse supersession into `HandshakeFailed`/`Expired` — turning an
attacker-triggered nuisance into a stream of apparent authentication failures, or
vice versa, silencing real ones.

**Fix.** `AuthError::{Replay, HandshakeFailed, Expired, Superseded}` and an
explicit `AcceptError::{Expired, Superseded, Internal, ConnectionLost, …}`
enumeration, with a one-line note that `HandshakeFailed` is the only variant that
represents a cryptographic failure and is the only one that should feed a
security signal.

---

### 10. MAJOR — supersession leaves it undefined whether the re-entered stage-0 initiation is surfaced; both readings are harmful

**Where.** §4 *Supersession*, lines 210–215; §6 `EndpointOutput::IntroReady`,
line 448.

**What the text says.**

> the mid-state […] is discarded, the fresh initiation **re-enters at stage 0**

**The gap.** `IntroReady(IntroId, SocketAddr)` is defined as "stage-0 arrival for
the accept queue". The draft never says whether a re-entering initiation emits a
new `IntroReady`. Both readings are bad and the choice is security-relevant:

- **It does emit.** Then each attacker packet (finding 3) produces a fresh
  `Intro` handed to the application. One source address yields an unbounded
  stream of `Intro`s — the queue's per-address dedup bounds *slots*, not
  *surfacings*, so the accept channel becomes the unbounded thing the receive
  buffer was bounded to avoid (§8, lines 681–683).
- **It does not emit.** Then a genuine peer whose initiation superseded an
  in-progress chain is parked and **never surfaced again** — it ages out at
  `INTRO_TTL` and the peer can never connect while any chain for that address is
  live.

**Fix.** Specify it, and bound it: with finding 3's fix (a consumed chain owns its
own `IntroId`), a replacement is a genuinely new stage-0 entry and *does* emit
`IntroReady` — and the per-address slot bound then also bounds surfacings,
because the new entry occupies the address's one slot. State that a given source
address can hold at most one *unconsumed* stage-0 entry and therefore at most one
outstanding un-popped `IntroReady`.

---

### 11. MAJOR — "a failed completion spends the attempt" makes a one-packet-per-5-s handshake denial normative, and it is absent from the DoS accounting

**Where.** §5 *Initiator pendings*, lines 335–338.

**What the text says.**

> **One completion attempt per retransmit interval**: the pending's attempt
> state is *taken* on the first index-matching msg2; a second msg2 in the same
> interval is dropped. A failed completion spends the attempt; the next
> retransmit refreshes it.

**The attack.** `sender_index` rides in **cleartext** in the HandshakeInit header
(v1 §3), and msg2's mac1 is keyed on the **initiator's** static (v1 §4), which is
public data (A2). So an attacker who observes one msg1 — or who already knows the
initiator's static and can observe the index — can mint a mac1-valid,
index-matching, garbage HandshakeResp. The Noise read fails, **the attempt is
spent**, and the genuine msg2 arriving microseconds later is dropped ("already
consumed this interval"). Repeat once per retransmit interval (~5.3 s) and the
handshake never completes: `Connecting` resolves `Err(ConnectError::TimedOut)` at
90 s. Cost to the attacker: ~17 packets per denied connection.

This is **v1's implemented behaviour** (`src/endpoint.rs:762–780`: the attempt is
`take()`n before `complete_init` and is not restored on failure) and is forced by
hiss's consume-`self` state machines — so it is not a regression. But the draft
*promotes it from an implementation artefact to normative protocol* and does so
in the same section that claims a complete per-packet DoS accounting, without
listing it. An off-path attacker cannot do this (they must guess a 32-bit index);
a passive on-path observer can.

**Fix.** Keep the rule (it is forced), but state the exposure where the
accounting lives: add a line to §5 noting that a mac1-valid, index-matching but
cryptographically invalid msg2 costs the initiator its completion attempt for
that interval, that this is an on-path (index-observing) capability, and that the
mitigation — retaining enough state to retry a failed read — is foreclosed by
hiss's consuming state machines. If a cheap partial mitigation is wanted:
**re-arm the retransmit immediately on a failed completion** rather than waiting
out the interval, which reduces the attacker's leverage from "denies the
handshake" to "adds one round trip per forged packet".

---

## MINOR findings

### 12. MINOR — §8's "nothing is lost or duplicated" overstates the safety of the instant swap cut

**Where.** §8, lines 608–623.

**Text.** "undelivered messages re-queue onto the fresh session (v1 §9.5) […] so
nothing is lost or duplicated; the only unrecoverable casualties are keepalives,
which are expendable."

**Gap.** True only when the replacement completes end-to-end. The responder
installs the replacement session and drops the old index/keys the moment it
*writes* msg2 (v1 §5 Responder 6, "session live immediately"); the initiator
installs only on *receiving* msg2. If msg2 is lost, the responder can no longer
open the peer's old-session traffic and the peer cannot open the responder's new
traffic — the connection is dark until the next initiation (~5 s), against a 15 s
liveness budget. An active on-path attacker who drops only HandshakeResp packets
(a strictly cheaper filter than dropping everything) converts a routine rekey
into a teardown. Inherited from v1, but the claim as written is wrong.

**Fix.** Qualify: "…nothing is lost or duplicated **once the replacement
completes on both sides**; a lost msg2 leaves the connection one-way-dark until
the next initiation, against the 15 s liveness budget."

### 13. MINOR — the peer-restart limitation understates the failure: the swallowed messages are also ACKed

**Where.** §11, lines 806–813.

**Text.** "A peer that *restarts* […] re-sends seqs from 0, and we silently
swallow them as duplicates."

**Gap.** ACK ranges are built from the **replay window** (v1 §9.2), not from
delivery. A restarted peer's DATA frames arrive on fresh counters, are admitted
to the window, are ACKed, and are then dropped by seq dedup. The peer's recovery
layer clears them as delivered. So the loss is not merely silent on our side — it
is **affirmatively confirmed as delivered to the sender**, unrecoverable and
undetectable by either end. That is a materially stronger statement than "we
swallow them" and belongs in a clause labelled a known limitation.

**Fix.** Add: "…and, because ACK ranges are built from the replay window, these
messages are **acknowledged** — the restarted peer's recovery layer clears them
as delivered. The loss is silent, confirmed, and undetectable at both ends."

### 14. MINOR — the amendment silently drops v1's ratified acknowledgement of the membership-timing oracle, and `IntroError::Internal` sharpens it

**Where.** §2 table row for v1 §5 (line 49); §4 typestate table; §5 step 4
(lines 269–272).

**Gap.** §2 says v1 §5's Responder step list is "amended by §§4–5". v1 step 4
ratified an explicit acceptance: "an accepted ~one-ECDH timing difference that a
prober could use to test a public key's allow-list membership (statics are public
data; accepted — ratified 2026/07/16)". Nothing in §4 or §5 restates it, so its
status under the amendment is undefined. Meanwhile v2 makes the same distinction
*sharper*: probing a claimed static that is a known static costs an extra `ss` and
returns `IntroError::Internal`, while an unknown one returns `Claimed(S)` at one
DH less — the same oracle, now with an explicit API-level discriminator and a
larger timing gap. The oracle's *population* also changed, from "the configured
allow-list" to "currently-connected or currently-dialled statics", which leaks
liveness rather than configuration.

**Fix.** Restate the acceptance in §4 or §5, updated for v2: the probe now tests
membership of the *live* known-static set (a liveness oracle, not a configuration
oracle), it is exposed both as timing and as the `Internal`/`Claimed`
discriminator, and it remains accepted — or re-ratify it.

### 15. MINOR — the "nothing durable may be keyed on it" warning covers only the claimed static

**Where.** §4, lines 139–147.

**Text.** "The claimed static at `Claimed` is attacker-choosable […] nothing
durable may be keyed on it — no map insertion, no rate-limit bucket, no unbounded
logging."

**Gap.** `Intro::source()` and `Intro::sender_index()` are exposed at **0 DH**
and are equally attacker-chosen (the source may be spoofed; `sender_index` is
cleartext attacker-supplied). An application that builds a per-source rate-limit
map or logs per `sender_index` from stage 0 has an attacker-controlled allocation
primitive that the warning, as scoped, does not forbid.

**Fix.** Extend the sentence to all stage-0 accessors: "the claimed static, the
source address, and `sender_index` are all attacker-chosen; nothing durable may be
keyed on any of them."

### 16. MINOR — the pacing gate's scope is under-specified relative to the continuation's own trigger set

**Where.** §5 pacing, lines 303–313; §5 step 3/step 4, lines 260–272.

**Gap.** Pacing is defined as "per-established-peer" and scoped to "the internal
continuation only". But the continuation also runs for **pending outbound
remotes** ("known statics = established connections ∪ pending outbound remotes")
and via the **`read_identity()` interception**, neither of which involves an
established peer. Undefined: whether an initiation matched to a pending-outbound
remote is paced, and against what counter (the guard map's value, which for a
never-yet-connected peer may not exist).

**Fix.** Re-scope to "per known static" and say that the counter lives in the
guard entry for that static (which §7 already pins for in-flight pendings), so
the counter exists for every continuation trigger.

### 17. MINOR — the total bound on endpoint state is never stated, and connection count is uncapped

**Where.** §4 (queue cap, mid-states), §7 (guard pinning + orphan cap), §8
(`RECV_BUFFER`).

**Gap.** Each bound is stated in isolation; the composite is not, and one term is
missing entirely:

- Mid-states: ≤ `INTRO_QUEUE_CAP` × ~0.5–1 KB ≈ **1 MB of live key material**,
  each holding the endpoint's static provider. For a hardware/enclave-backed
  static that is **1024 concurrent provider handles** — a resource the draft's
  own §1/§12 rationale treats as scarce, and one an attacker can drive to the cap
  (finding 2) at 1024 DH of our cost.
- Guard map: `TS_GUARD_ORPHAN_CAP` (1024) **plus** every pinned entry — pinned
  entries include one per staged mid-state (≤1024) and one per connection, which
  is uncapped.
- **There is no cap on the number of established `Connection`s.** At
  `RECV_BUFFER = 256` messages × `MAX_MESSAGE` 1159 B, a single connection's
  receive buffer is up to **~297 KB**, so connection count is the dominant memory
  term and it is governed only by application policy.

**Fix.** Add a short composite table ("endpoint state ceilings") giving each term
and naming connection count as application-governed and unbounded by the
protocol, with the per-connection receive-buffer worst case spelled out so an
application can size its accept policy.

### 18. MINOR — an authenticated peer can place an arbitrary address into the hint set

**Where.** §5 hint set definition, lines 254–257; §8 *Roaming observability*,
lines 656–665.

**Gap.** The hint set is derived from connections' *current endpoint addresses*,
and roaming moves that endpoint on an authenticated fresh Data packet whose
**source address is not itself authenticated**. So any key-holding peer can roam
its own connection to an arbitrary address `X` (spoofing its source), which
inserts `X` into the hint set and grants the eager path (1 DH per packet) to
whoever sits at `X`. The gain over the alternatives is marginal (1 DH vs 0 DH)
and the peer black-holes its own connection doing it, but the claim that hint-set
entry "requires knowing an established connection's current 4-tuple" is not the
complete entry condition.

**Fix.** Note it alongside finding 6's restatement: hint-set membership can be
*obtained* by any key-holding peer via a spoofed-source roam, not only *guessed*.

---

## NIT findings

### 19. NIT — state the continuation's step order as a numbered list

§5, lines 290–301, states the continuation as one prose sentence. Two
security-critical orderings are buried in it: (a) guard **and** pacing are
checked *before* any index is minted, any msg2 is written, or any session is
installed; (b) the pending cancellation happens *on install*, i.e. strictly after
authentication. Both are correct as written — the review confirms the H2
cancellation cannot be triggered by a forged claim (dies at `ss`) or by a replayed
msg1 (dies at the guard) — but a numbered list makes them unmissable and
un-driftable.

### 20. NIT — name the failed-open case explicitly

§7.3's index re-draw closes cross-connection *routing* confusion, and it is
strong enough as written (re-draw against **both** tables, and the pending index
graduating into the session index is safe because it is present in the pending
table from mint time). What is never said is the corollary: a datagram that
routes by index but **fails to open** must not touch liveness, roaming, or the
replay window. It follows from §8's decrypt-first ordering, but a freed-then-
re-drawn index makes stale traffic land on the wrong connection routinely, so the
invariant deserves one sentence.

### 21. NIT — record the absence of amplification

The accept path replies 107 B (msg2) to a 196 B stimulus (msg1), and no other
path emits bytes in response to an unauthenticated packet. **Silent-drop
discipline verified**: no path in this draft transmits where v1 would not — no
cookie replies, no error packets, `0x04` still never emitted, and every staged
rejection is local. This is a genuine property worth stating in §5's DoS section
rather than leaving to inference.

### 22. NIT — `slither::policy` outlives the policy it traced

§10 (lines 779–781) keeps `slither::policy` as operator-visible contract, but the
allow-list it traced is dissolved. Say what it traces now (presumably guard
rejections, pacing rejections, and continuation outcomes) — operators build
alerts on these targets, and a target that silently changes meaning is worse than
one that is renamed.

---

## Honesty-clause audit

| # | Required clause | Present? | Accurate? |
|---|---|---|---|
| 1 | Guard-eviction replay | **Yes** — §7, lines 591–599, labelled and `[MAINTAINER]` | **Partially.** The mechanism, the WireGuard quotation, and the pinning guarantee are all accurate (pinning does soundly protect established connections). **Omits** that eviction is attacker-triggerable on demand, that LRU order preferentially evicts the longest-idle legitimate peers, and that the accept-path consequence is a spurious `Connection` attributed to a real peer. See finding 8. |
| 2 | Spoofed-source queue exposure | **Yes** — §4, lines 230–235, labelled | **No.** Overstates the required capability (no spoofing needed — distinct source *ports* suffice), understates the consequence (endpoint-wide denial of **all** inbound accepts, not just memory), omits the sustaining rate (~11 pps), and the "WireGuard-equivalent" comparison is wrong (WireGuard's ring is drained at line rate and is backed by the under-load cookie gate and a per-IP token bucket). See finding 2. |
| 3 | 128-window reordering cost | **Yes** — §8, lines 640–654 | **Yes.** The derivation chain (128-bit window → `MAX_ACK_RANGES = 63` → 268 B maximal ACK → 0.1.0 peers reject >63 ranges) is correct, the reference comparison (kernel 8128 usable, boringtun 1024) matches research-wireguard §5, and the cost is stated in the right terms: "reordering beyond 128 packets drops the stragglers at the window, which Leg 2 converts into spurious retransmissions — not loss." No changes needed. |
| 4 | Peer-restart known limitation | **Yes** — §11, lines 806–813, labelled `[MAINTAINER]` | **Nearly.** Correct that rekey and restart are cryptographically indistinguishable at msg1 time, that the timestamp-gap heuristic is unsound, and that the sound fix is wire work. **Understates** the failure: the swallowed messages are ACKed from the replay window, so the sender clears them as delivered. See finding 13. |

Two of the four are inaccurate in ways that matter for the decisions the clauses
exist to inform.

---

## Verdict

**SOUND-WITH-FIXES** — but four of the findings are ratification-blocking, and
two of those (findings 2 and 3) require a maintainer design ruling rather than a
wording change.

The architecture survives adversarial reading. The three load-bearing security
decisions are correct as designed: **hint-set routing genuinely preserves the
0-DH off-path drop in the endpoint core** and cannot cause cross-connection
confusion (the continuation matches on the *claimed static*, never on the source
address); **the internal continuation is a faithful reproduction of v1's
responder tail**, and its claim of 2-DH-equals-v1's-exposure holds — indeed v2's
"known statics" population is narrower than v1's allow-list, so the exposure is
strictly smaller; **guard admission is correctly placed post-`ss` at both
admission points**, so only key-holders write guard entries, and the pinning rule
is airtight for established connections (no unauthenticated action can demote a
pinned entry to an orphan). The H2 cancellation is correctly gated on *install*,
so neither a forged claim nor a replayed msg1 can cancel a genuine pending. Index
re-draw across both tables is stated strongly enough. Silent-drop discipline is
intact: no path emits bytes in response to an unauthenticated packet where v1
would not, and the accept path does not amplify.

What must be fixed before ratification:

1. **Finding 1** breaks a ratified v1 invariant that §2 claims to carry "in
   full", and is exploitable by a passive observer into endpoint hijack. Wording
   plus a 16-byte core change.
2. **Finding 4** is a guaranteed mutual blackout on a one-RTT-wide window in the
   design's own target topology, and the stated resolution rule is factually
   wrong. Needs a deterministic tie-break.
3. **Finding 2** needs a policy ruling (evict-oldest, per-source cap, shorter
   parked TTL — pick two) and a rewritten honesty clause.
4. **Finding 3** needs a ruling on whether a consumed chain owns its bytes.
5. **Finding 5** silently reintroduces the exact `es, es, ss` cost shape §1
   invokes to justify killing the fallback; it must be fixed or the ratified cost
   table amended.

The DoS accounting table (§5 B4) does not survive re-derivation as written: one
row is missing, one is wrong about what the pacing gate caps, the headline
conclusion holds only for a degenerate application, and the claimed 2-DH ceiling
is exceeded on the demote-then-probe path. It is repairable — the *bound* is
recoverable at 2 DH per attacker packet once finding 5 is fixed — but it should
not be ratified in its current form.

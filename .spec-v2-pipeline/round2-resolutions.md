# Round-2 resolutions — SPEC-v2-DRAFT.md

Consolidated from three reviews (`review-security.md` = **[sec]**,
`review-implementability.md` = **[impl]**, `review-consistency.md` = **[con]**).
Each finding is ruled here with the concrete edit. The reviser applies these to
produce `SPEC-v2-DRAFT-v2.md`. Rulings marked **[MAINTAINER]** are posture
choices the maintainer should confirm; I give the recommendation and the draft
proceeds on it.

The three reviewers agreed the architecture is sound — hint routing preserves
the 0-DH off-path drop and cannot cross-wire connections, the internal
continuation faithfully reproduces v1's responder tail, guard admission is
correctly post-`ss`, silent-drop discipline holds with no amplification. What
follows repairs the edges, not the core.

---

## GROUP 1 — Receive backpressure: restore the replay invariant ([sec] BLOCKER 1, [con] MAJOR 4, [impl] MAJOR 7)

**The hole.** The shed rule marked the replay window only on *delivered*
packets, driving liveness/roaming off *unmarked* fresh packets. A passive
observer can replay a recorded shed Data packet from a spoofed source and roam
the endpoint to themselves — breaking v1 §6's ratified "no replayed packet ever
moves the endpoint," which §2 claims to carry in full. It also suspends replay
detection under sustained backpressure and is packet-vs-message ambiguous.

**Ruling.** Restore the v1 invariant: **mark the replay window on every
authenticated, fresh packet — shed or delivered.** Liveness and roaming are
driven *only* by a packet that is both authenticated and window-marked. To keep
a shed packet from being falsely ACKed, add a second 128-bit `shed_mask`
alongside the replay window: a shed counter is marked in the window (replay
protection holds) and its bit is set in `shed_mask`; ACK-range construction
subtracts `shed_mask` from the window snapshot, so a shed counter is never
acknowledged. The peer's loss detection retransmits the frames on a *fresh*
counter (a new window bit), delivered when the buffer drains; exactly-once holds
via seq dedup. Cost: 16 bytes/connection, a pure-core change.

Granularity: **"room" means room for every DATA frame aboard the packet.** If
the buffer cannot hold all frames in the packet, the whole packet is shed
(marked in window + `shed_mask`, never delivered, never ACKed) — never
partially delivered. Sequence: decrypt → window **check**; already-seen ⇒ drop
(v1) → fresh ⇒ window **mark** + liveness + roaming → room for all frames?
deliver + schedule ACK : set `shed_mask`, no delivery.

§2's roaming row must stop claiming v1 §6 survives "in full" and name the
observability + shed carve-outs. State the shed exposure honestly: under
sustained backpressure a shed packet is protected against replay (window
marked), so the [impl] MAJOR 7 "replay suspended" concern is *resolved by this
ruling*, not merely documented.

---

## GROUP 2 — Simultaneous open must converge ([sec] BLOCKER 4)

**The hole.** H2 cancels the local pending on install; each side's *inbound*
handshake completes first, so both install different key sets and go mutually
dark for 15 s. "Resolves to whichever completes first" is false under local
cancellation. This is the design's own target topology (mutual autoconnect).

**Ruling.** Replace the race with a **deterministic tie-break on the static
keys**, evaluated identically on both sides. When an inbound initiation's
claimed static matches a peer to whom we hold an in-flight outbound pending
(known the moment the eager-path `es` yields the claimed static — which for a
dialled peer we already hold as the dialled remote), **the peer with the
lexicographically smaller static public key is the winning initiator**:

- our static smaller ⇒ we are the winning initiator: **silently drop the
  inbound msg1** (discard its mid-state; no `ss`, no msg2, no guard record) and
  let our own outbound complete;
- peer's static smaller ⇒ we cancel our pending and run the continuation as
  responder (drafted behaviour).

Both sides compare the same ordered pair and reach complementary conclusions, so
exactly one session (`{msg1_winner, msg2_loser}`) is built and both hold it.
Decided at the claimed-static match, before the second DH. Delete "resolves to
whichever handshake completes first." The comparison uses the 33-byte compressed
encodings (the mac1-keying bytes), compared as unsigned big-endian octet
strings; a self-connection (equal statics) is impossible under B1 (`connect` to
our own static is out of scope; note it).

Interaction with H2's cancel-on-install: cancellation now happens at the
tie-break decision (pre-`ss`) when we lose, or the continuation's install when
we win-as-responder — restate the numbered continuation order accordingly
([sec] NIT 19).

---

## GROUP 3 — Stage-0 queue DoS ([sec] BLOCKER 2) **[MAINTAINER]**

**The hole.** Dedup on full `SocketAddr` (incl. port) + drop-incoming overflow +
90 s TTL ⇒ one host fills all 1024 slots via 1024 source *ports* (no spoofing),
denying **all** inbound accepts endpoint-wide at ~11 pps. The "WireGuard-
equivalent" clause is wrong (WG's ring drains at line rate + cookie gate +
per-IP token bucket).

**Ruling (recommend all three cheap mitigations, [MAINTAINER] to confirm):**

1. **Overflow = evict-oldest** (by park time), not drop-incoming, so a genuine
   initiation always obtains a slot; the attacker must win a per-packet race,
   not hold a permanent reservation.
2. **Per-source cap** `INTRO_MAX_PER_SOURCE = 4` parked entries per source IP
   (per /64 for IPv6), the 0-DH analogue of WG's per-IP token bucket — kills the
   single-host-many-ports variant. Applies to **unconsumed** stage-0 entries
   only (attacker-controlled); consumed chains are app-driven (Group 4).
3. **Shorten the parked TTL** to `INTRO_TTL = 15 s` (was 90 s). 90 s was the
   *initiator's* give-up horizon, not the responder's obligation; entries are
   superseded every ~5.3 s in normal operation, so 15 s is ample and cuts the
   hold cost ~6×. (Note: this also tightens the [sec] finding-8 orphan concern
   in Group 8.)

Rewrite the honesty clause: (i) no spoofing needed — distinct source ports
suffice; (ii) filling the cap denies *all* new inbound accepts endpoint-wide,
not just memory; (iii) the sustaining rate; (iv) this is **not** WireGuard-
equivalent — WG drains at line rate and adds the under-load cookie gate and a
per-IP rate limiter, of which slither has only the per-source slot cap until the
deferred cookies/mac2 round. Keep the constants as configurable defaults with a
draft-note that the flood-test validation is a phase-1 implementation task
([con] MAJOR/round-2 items 5–6).

---

## GROUP 4 — Supersession poisoning → own-bytes-on-consume ([sec] BLOCKER 3, [con] BLOCKER 1, [impl] MAJOR 14, [sec] MAJOR 10)

**The hole.** Dedup on source address alone lets one unauthenticated mac1-valid
packet clobber a victim's in-progress (DH-paid) staged chain at 0 DH — a
regression vs v1. And `Superseded` is unrepresentable in `AuthError`/`AcceptError`.

**Ruling — adopt the own-bytes model (strictly better than adding the variant):**

- A parked entry is **unconsumed** until the app calls `read_identity()`. While
  unconsumed, a newer initiation from the same source **transparently replaces**
  its bytes and refreshes the TTL (same `IntroId`, newest bytes; accessors
  reflect newest bytes at call time; **no** second `IntroReady` for an
  already-surfaced entry — bounds surfacings to one per source). The app has
  spent nothing, so transparent replacement is harmless.
- The moment `read_identity()` is called, the **chain owns its bytes and its
  `IntroId`**; the stage-0 slot for that source is freed. A subsequent
  initiation from that source parks as a **new** stage-0 entry (new `IntroId`,
  new `IntroReady`). A consumed `Claimed`/`Proven` chain can therefore **never**
  be superseded by any later packet.

**Consequence: `Superseded` becomes unreachable — remove it from every error
enum.** No verb can observe it: stage-0 replacement is transparent (no
Superseded at `read_identity`), and consumed chains are isolated (no supersession
after). This resolves [con] BLOCKER 1 and [impl] MAJOR 14 by deletion rather than
addition, and closes the [sec] BLOCKER 3 attack (an unauthenticated packet cannot
touch a consumed chain). Supersedes both reviewers' "add Superseded to AuthError"
fix — note the divergence and the reason (own-bytes removes the attack *and* the
variant).

State the state-accounting: total accept-side state = `INTRO_QUEUE_CAP`
unconsumed stage-0 entries (Group 3 caps/TTL) **plus** live consumed chains the
app holds (bounded by the same cap: a consumed chain holds a slot until the app
drops its handle; advancing each cost ≥1 DH and is app-driven). The per-source
cap (Group 3) applies only to unconsumed entries.

Edge to note: the app may briefly hold a `Claimed` (from retransmit N) and a new
`Intro` (retransmit N+1) for the same peer; it resolves one, and B1 makes the
second `accept()` return `AcceptError::AlreadyConnected`.

---

## GROUP 5 — Eager-demote must not cost 3 DH ([sec] MAJOR 5)

**The hole.** Eager path pays `es`, finds the claimed static unknown, **discards**
the mid-state and re-parks; the app's `read_identity()` re-pays `es`, then
`authenticate()` pays `ss` = 3 DH per packet — the exact `es,es,ss` shape §1
invokes to kill the fallback, and it breaks the §4 cumulative table.

**Ruling — carry the paid mid-state (option a).** An eager-path initiation from a
hint-set source whose claimed static is unknown is **not** discarded: the parked
entry keeps its mid-state, tagged "identity already read." When the app calls
`read_identity()` on it, the endpoint returns the **cached** claimed static at
**0 incremental DH**. Marginal cost stays `es`(1) → `authenticate` `ss`(+1) →
`accept` `ee,se`(+2); per-attacker-packet cost for this class is **1 DH**, not 3;
the ratified cumulative table (Claimed 1 / Proven 2 / Connection 4) holds. Bound:
such carried entries are keyed by hint-set sources (≤ connections + pendings) and
count against the queue cap like any parked entry. Add a footnote to the
typestate table: a hinted-source entry may arrive with its identity pre-read (the
1 DH is charged at the eager read; the app's `read_identity()` is then free).
Update the DoS table row accordingly (Group 7).

---

## GROUP 6 — accept()/Install and HandshakeFailed routing ([impl] BLOCKER 1, [impl] MAJOR 2)

**Ruling.** `accept()`'s returned `core::Connection` is **fully established** —
the session is baked in at construction (all 4 DHs paid, msg2 queued). **`poll_output()`
never emits `Install` for a `ConnectionId` surfaced via `accept()`.** `Install`
events target only:
- a `connect()`-created (initiator) connection awaiting completion —
  `Install{initial: true}`, exactly once (resolves its `Connecting`);
- any established connection being rekeyed — `Install{initial: false}`, silent swap.

**`HandshakeFailed` never reaches `core::Connection`.** It is an endpoint-level
output the shell consumes to resolve the `Connecting` future with
`Err(ConnectError::TimedOut)` and then drop the never-established pending
connection handle. Narrow `handle_endpoint_event` to carry only
`Install{session, initial}` (not the whole `EndpointToConn`); `HandshakeFailed`
is a distinct `EndpointOutput` variant the shell routes to the `Connecting`
waker, not to a connection core. State both rules explicitly in §6.

---

## GROUP 7 — Error taxonomy, consolidated ([sec] 9, [impl] 3/13, [con] 3/11)

Single normative taxonomy in §10; every variant named anywhere appears here once.

- `IntroError::{Expired, Internal, Malformed, EndpointDropped}` — **Superseded
  removed** (Group 4). `Internal` = the B2.4 interception (consumed by the
  endpoint; discard). `EndpointDropped` = driver gone mid round-trip.
- `AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}` — Replay =
  automatic guard failure; `HandshakeFailed` = the tail-tag/crypto failure (the
  **only** variant that is a security signal — state this); Expired = the
  parked attempt aged out at this stage; **Superseded removed**.
- `AcceptError::{Expired, AlreadyConnected, EndpointDropped}` — newly enumerated
  (was undefined). AlreadyConnected = a session for this static installed
  meanwhile (B1). No Superseded.
- `ConnectError::{AlreadyConnected, TimedOut}` — **closed** for phase 1 (was an
  open ellipsis).
- `ConnectionLost::{TimedOut, RekeyFailed, NonceExhausted, LocallyClosed,
  EndpointDropped}` — gloss each: TimedOut = liveness (15 s); RekeyFailed = the
  180 s payload backstop; NonceExhausted = seal nonce exhaustion (H1);
  LocallyClosed = `close()` or last-handle drop observed by a surviving clone;
  EndpointDropped = driver stopped.
- `SendError::{PayloadTooLarge, ConnectionLost(ConnectionLost)}`.

§10's sentence must name **all three** staged error types (Intro/Auth/Accept),
not just two.

---

## GROUP 8 — Security honesty + DoS-table repairs ([sec] 6/7/8/11/12/13/14/15/17/18)

**DoS table (§5 B4) — rebuild so it survives re-derivation:**
- Fix the demote-then-probe row to **1 DH** (Group 5).
- Add a row: **replayed genuine msg1 from a hint-set source** (recorded once,
  replayed at line rate) → **2 DH** (`es`+`ss`), dies at the timestamp guard,
  **not** capped by pacing (pacing gates acceptance after both DHs). This is the
  cheapest sustained 2-DH primitive; it must appear.
- Split the "genuine replacement, 4 DH capped" row into: **2 DH per initiation
  (uncapped)** + **2 DH only for the ≤ `INITIATIONS_PER_SECOND` accepted**.
- State the explicit ceiling: **max cost of any single attacker packet is 2 DH**,
  and only 1 of those is reachable without the attacker holding a hint-set
  address or the application choosing to spend.

**Off-path conclusion ([sec] 6):** replace "off-path attackers do strictly
better (0 DH)" with a capability statement: an attacker who cannot forge source
addresses pays **0 DH in the endpoint core**, and **1 DH (probe) or 2 DH (probe +
authenticate) per packet against any application that implements a policy**, plus
one parked slot per distinct source. Fix "4-tuple" → "the source `(IP, port)` of
any established connection **or any address we have dialled**"; state the entropy
honestly (a peer on a well-known dialled port ≈ 0 bits). Note the hint set is
also *obtainable* by any key-holding peer via a spoofed-source roam ([sec] 18),
not only guessable.

**Guard-eviction clause ([sec] 8) [MAINTAINER]:** complete it — (i) eviction is
attacker-triggerable on demand (≈1024 authenticate-then-drop chains with
self-generated statics; ~2048 DH of *our* cost); (ii) LRU evicts the longest-idle
legitimate peers first; (iii) the observable is a spurious `Connection`/`Intro`
attributed to a real peer at an attacker-chosen address (dies at 15 s liveness,
no key leak). **Mitigations ruled in:** do **not** create an orphan entry for a
static authenticated-then-rejected-without-accept; and **age orphans out on a
timer** (`INTRO_TTL`-scale) as well as the LRU cap, so attacker volume does not
translate into eviction of durable entries. Define LRU "use" ([con] 10): **only
admission (the post-`ss` record) refreshes recency** — a failed guard *check*
does not, keeping the write path key-holder-only.

**attempt-spend ([sec] 11, [con] 18):** add to §5's accounting that a mac1-valid,
index-matching but cryptographically invalid msg2 spends the initiator's
completion attempt for that interval — an **on-path (index-observing)** capability
(off-path must guess a 32-bit index), whose full mitigation is foreclosed by
hiss's consuming state machines. **Rule the cheap partial mitigation in: re-arm
the retransmit immediately on a failed completion** (rather than waiting out the
interval), reducing the attacker's leverage from "denies the handshake" to "adds
one round trip per forged packet." Tighten the precondition: the attempt is
taken on the first **length-correct, index-matching, mac1-valid** msg2.

**swap-cut qualification ([sec] 12):** "nothing is lost or duplicated **once the
replacement completes on both sides**; a lost msg2 leaves the connection
one-way-dark until the next initiation (~5 s), against the 15 s liveness budget."

**peer-restart clause ([sec] 13):** add that the swallowed messages are **ACKed**
from the replay window, so the restarted peer's recovery layer clears them as
delivered — the loss is **silent, confirmed, and undetectable at both ends.**

**membership-timing oracle ([sec] 14) [MAINTAINER]:** v1 §5 step 4 ratified the
~one-ECDH allow-list-membership timing oracle as accepted. v2 changes its
population (now the *live* known-static set — a liveness oracle, not a config
oracle) and adds an explicit API discriminator (`IntroError::Internal` for a
known static vs `Claimed` for an unknown one, at one DH less). Restate the
acceptance in §4/§5 updated for v2, or re-ratify.

**stage-0 accessor warning ([sec] 15):** extend "nothing durable may be keyed on
it" to **the claimed static, the source address, and `sender_index`** — all
attacker-chosen at stage 0.

**endpoint state ceilings ([sec] 17):** add a short composite table — mid-states
≤ `INTRO_QUEUE_CAP` × ~1 KB live key material (for a hardware static, **1024
concurrent provider handles** — call this out); guard map = orphan cap + pinned
(one per mid-state ≤ cap, one per connection); **connection count is
application-governed and unbounded by the protocol**, per-connection receive
buffer worst case `RECV_BUFFER` × `MAX_MESSAGE` ≈ 297 KB.

**pacing scope ([sec] 16, [con] 9):** re-scope pacing to **per known static**,
counter stored in the guard entry (which §7 pins for in-flight pendings, so it
exists for every continuation trigger); mechanism = **minimum 20 ms spacing
between accepted replacements per peer**.

**continuation order ([sec] 19, [con] 8):** state the continuation as a numbered
list; order = **tag → guard → pacing**; record the greater timestamp **only on
full admission**; the silent-drop-connection-untouched clause covers **all three**
(tag, guard, pacing) failures.

**NITs:** record the no-amplification property in §5 ([sec] 21); a datagram that
routes by index but fails to open touches neither liveness, roaming, nor the
window ([sec] 20); say what `slither::policy` traces now (guard/pacing rejections
+ continuation outcomes) ([sec] 22).

---

## GROUP 9 — Mechanical / definitional fixes

- **`Timeout(None)` ([con] 2):** define — `Timeout(None)` = drained, no deadline
  armed; `Timeout(Some(d))` = drained, next deadline `d`; `L` applies to `Some`.
  Same semantics both cores. Fix the §6 quote `Timeout(Instant)` →
  `Timeout(Option<Instant>)`.
- **keepalive-vs-teardown order ([impl] 4):** **invert the bullet** — teardown
  collection precedes keepalive; a session collected for teardown owes no
  keepalive (matches `endpoint.rs:904-925`; reachable at the 25 s default).
- **`core::Endpoint<I: Identity>` ([impl] 10):** the constructor/type is generic
  over `I: Identity` (mirroring v1's `Actor<I, W>`); the mid-state map is
  `HashMap<IntroId, …<I::Provider>>`. Not `identity: Identity` bare.
- **`send_unreliable` seq ([impl] 6):** draws from the **same** monotonic seq
  space as `Reliable::send` (skipping the retransmission-queue insert); add the
  `Recovery` "allocate-seq-without-tracking" primitive to the surface; add a
  reliable/unreliable interleaving test to §13's obligations.
- **synchronous sealing ([impl] 12):** state that plan-seal-commit (including
  `on_packet_sent`) executes **synchronously within the mutating call**
  (`handle_timeout`/`handle_datagram`/`send`), not lazily in `poll_output()`, so
  `handle_timeout` idempotency holds (no double `on_pto` / backoff corruption).
- **deadlock invariant ([impl] 5):** state the general rule — all driver→handle
  delivery is either a bounded channel with an explicit non-blocking shed policy
  (receive buffer) or a oneshot reply that cannot block the driver; accessors
  (`remote_static/remote_address/session_id/is_established`) are **synchronous
  reads of a shared cell the driver updates**, not round-trips.
- **`Retired` MUST ([impl] 11):** every terminal `ConnOutput` (a `Closed` event
  or `close()`) is followed within the same drain by `ToEndpoint::Retired`; the
  shell delivers it to `handle_connection_event` before releasing the
  connection's shell-side bookkeeping (frees the index route and demotes the
  guard entry pinned→orphan).
- **handshake.rs legacy functions ([impl] 8, [impl] 9) [MAINTAINER]:** rule that
  `accept_init`/`build_init`/`complete_init`/`RespAccept` are **replaced** by the
  split-read staged primitives the core needs; the four tests that call
  `accept_init` directly (`golden_wire_is_byte_identical…`, `msg_sizes_match…`,
  `tampered_payload…`, `responder_dh_cost_is_staged`) are **reclassified from
  survives-verbatim to harness-rewrite** — same golden/DH assertions, new call
  sites via the new primitives. Keep `ReplayWindow::admit` and
  `Recovery::next_packet` as thin wrappers over the new check/mark and
  plan/commit split primitives, so the `window_*` and `recovery::` unit tests
  need no change. Fix the recovery test count to **ten** ([con] 16).
- **§2 table completeness ([con] 6, 12, 7):** add a `§9.7` row (behavioural
  deltas — the `Incoming` language replaced by §10, restated by §9); dispose v1
  §8/§10 (out-of-scope — superseded by draft §11); extend the §6 row to name the
  §8 swap-cut and epoch-death amendments; carve out §9.2's `ack_delay` clock
  (the shell-supplied `now` under `L`, not tokio's).
- **round-2 item 4 ([con] 5) — RESOLVED:** I read `hiss/src/noise/datagram.rs`:
  the far-future-epoch refusal returns `HandshakeError::DecryptionFailed`,
  genuinely indistinguishable from any failed open. The draft's claim **stands,
  now verified** — cite `datagram.rs` and keep the "subsumed by liveness, no
  dedicated trace" framing. Not an open item.
- **placeholder constants ([con] 13):** keep `RECV_BUFFER`/`INTRO_QUEUE_CAP`/
  `TS_GUARD_ORPHAN_CAP` as configurable defaults with a one-line draft-note per
  table ("default pending phase-1 validation").
- **PERSISTENT_KEEPALIVE ([con] 17):** state the reclassification (25 s becomes
  the recommended default now that the interval is per-connection
  `Option<Duration>`; wire-invisible) rather than doing it silently.
- **pin-on-mid-state carve-out ([con] 15):** note the guard pin on a *claimed*
  (not yet proven) static is a bounded exception — it flips a bit on an entry a
  key-holder already wrote, reverting on drop, bounded by the queue cap.
- **lateness-bound single home ([con] 19):** §9 quotes the §6 `L` contract by
  reference rather than restating it.
- **`session_id()` ([con] 23):** define once ("the Noise handshake-derived
  session identifier the golden vectors pin").
- **next_counter() note ([con] 20):** Appendix A — the split read is
  codegen-only; `next_counter()` is a runtime accessor. Don't claim both are
  codegen-only.
- **DoS row-2 parse ([con] 21):** "application leaves the `Intro` unprobed (or
  drops it unprobed)."
- **pacing severability ([con] 24):** carry design B6's note that the pacing gate
  is severable if the maintainer wants a smaller phase 1.

---

## For the reviser

Apply every ruling above to produce `SPEC-v2-DRAFT-v2.md`, preserving the 13-
section skeleton, all DRAFT markers, house style, and the frozen-wire invariant.
Carry forward the existing 8 [MAINTAINER] flags and **add** flags at: Group 3
(queue DoS posture), Group 8 guard-eviction, Group 8 membership oracle, Group 4
(Superseded removal — diverges from both reviewers' literal fix), Group 9
handshake.rs-functions-replaced. At the end, list every change made against this
document's group numbers so the re-review can verify coverage.

# slither v0.2 phase 1 — design choices

Synthesis of 2026-08-13, for the SPEC-v2-DRAFT writer. Inputs: `TODO.md` (ratified
plan, FIXED), `SPEC.md` (ratified v1; wire untouchable), and the four research
reports in this directory (`research-actor-inventory.md` = **[actor]**,
`research-hiss-api.md` = **[hiss]**, `research-sansio-quinn.md` = **[quinn]**,
`research-wireguard.md` = **[wg]**).

**Fixed constraints honoured throughout** (not relitigated): IK-only; suite
genericity later via `Channel`; quinn-shaped object model; staged typestate
accept with drop-is-silent-reject and the ratified cost table (Intro 0 DH →
read_identity 1 DH → authenticate +1 DH → accept +2 DH); accept queue parks
stage-0 objects only; sans-io core recommended; ordering as a receiver-side
wrapper; streams/CC deferred; wire bytes frozen (golden-wire test green
throughout).

Decisions marked **[MAINTAINER]** need maintainer sign-off; everything else is
a technical ruling the draft may treat as settled.

---

## A. Split read is the plan of record; the fallback is dead **[MAINTAINER]**

**Ruling.** The spec's plan of record is the hiss native split read
(`read_message_1_intro(&msg1) → (claimed, MidRead)` + `MidRead::complete() →
([u8;12], RespMsg2)`), with **no fallback path in the spec or the code**.
Phase 1 gates on a hiss 0.3.x minor shipping it. The second hiss proposal,
`DatagramSend::next_counter() -> u64`, is **folded into the same hiss minor**.

**Rationale.**

- The fallback breaks two ratified DH-cost pins: the accepted msg1 read becomes
  3 DHs (`es, es, ss`) instead of the pinned 2, total responder cost 5 instead
  of 4 ([hiss] §3.3 table; [actor] hazard 5.11 — `responder_dh_cost_is_staged`
  and `unlisted_initiator_costs_one_dh_and_no_resp` both go red). Ratifying a
  new cost table for a temporary bridge is spec churn for nothing.
- The fallback perturbs the golden vectors: the throwaway stage-1 read costs an
  extra `Identity::provider()` draw, advancing the master CSPRNG and moving
  `GOLDEN_RESP`/`GOLDEN_SID` unless mitigated by provider cloning or a parallel
  one-shot path ([hiss] §3.5.1). All three mitigations are complexity spent on
  a path we intend to delete.
- Hardware statics pay the fallback twice per accepted handshake (each state
  rebuild is a `public_key` call — potentially an enclave round-trip; [hiss]
  §3.5.4).
- The split read is **zero runtime change in hiss** — pure `hiss-macros`
  codegen, ~300–450 additive lines, byte-compatible by construction because it
  re-uses the identical `support::*` calls in the identical order ([hiss]
  §2.2, §2.4). With the split, slither's staged path consumes exactly one
  provider draw per accepted handshake (same as today's `accept_init`), so the
  golden vectors are unmoved by construction.
- `next_counter()` deletes slither's mirrored counter + `debug_assert`
  ([hiss] §4.1). The spec describes the *behaviour* (the DataHeader carries
  exactly the counter the seal used, and the header is the AD); whether an
  implementation mirrors or reads the accessor is not normative.

**Details fixed for the hiss proposal** (record in the draft's appendix, not
the normative text): tail carrier is option (a), an owned
`[u8; __MSG1_TAIL_SIZE]` inside the mid state (no lifetime, no re-supply;
[hiss] §2.2); the claimed-static accessor is named `claimed_static()`; the
mid state is non-`Clone`; the `hiss::noise::Claimed<K>` newtype is
nice-to-have, not required — slither's own `Claimed` typestate carries the
claimed-vs-proven semantics regardless ([hiss] §2.3.3).

**Supersedes**: TODO.md §4's "usable to start phase 1 without blocking on a
hiss release" fallback option, and the open item "decide hiss split-read
timing". Marked **[MAINTAINER]** because it sequences work across two crates
and retires a ratified plan option.

---

## B. Rekey-vs-new-Intro routing

This is the load-bearing decision. Requirements, restated: (i) the application
must not re-approve — ideally not even see — a rekey of an established
connection; (ii) rekey processing must not depend on the app draining the
accept queue (an idle app must not kill its own connections at the 180 s
backstop); (iii) the 0-DH drop of an unwanted `Intro` survives; (iv) the
per-attacker-packet cost is bounded and stated; (v) the ratified 1/2/4 DH
stage costs on the app-visible accept path are preserved; (vi) stage-0-only
parking (raw bytes) survives.

### B1. One connection per peer static **[MAINTAINER]**

**Ruling.** The v1 invariant is ratified for v2: **one logical connection per
remote static, endpoint-wide**. The `static → Connection` map is endpoint-core
state (see D). `Endpoint::connect()` to a static that already has a live
`Connection` or an in-flight connect returns a typed error
(`ConnectError::AlreadyConnected`). An authenticated inbound initiation whose
static matches an existing connection is, by definition, a **session
replacement under that connection** (v1 SPEC §5 Responder 7), never a new
accept.

**Rationale.** Every routing rule below keys on this. Without it, "which
connection does this rekey belong to" has no answer, the timestamp guard's
per-static scope stops mapping onto connections, and v1's `static_to_conn`
collapse bug ([actor] hazard 5.1 — two connects to one peer silently
cross-wired) returns in a worse form. WireGuard's model is identical (one
`wg_peer` per static; [wg] §6). Marked **[MAINTAINER]** because it forecloses
multi-connection-per-peer for the 0.2 line and adds a public API error.

### B2. The routing rule: source-address-hinted eager path + read_identity interception

**Ruling.** Inbound `HandshakeInit` processing in the endpoint core:

1. **Stage 0 (always):** length gate, classify, mac1 verify. Fail → silent
   drop. Cost: one keyed hash. Unchanged.
2. **Hint check (no DH):** let the *hint set* = the current endpoint addresses
   of all established connections ∪ the dialled addresses of all in-flight
   outbound initiations. If `src` ∉ hint set → **park at stage 0** (raw
   ~196 B + addr, dedup per E) and surface an `Intro`. This is the ratified
   staged-accept path, unchanged, 0 DH until the app asks.
3. **Eager path (src ∈ hint set):** the endpoint immediately runs
   `read_message_1_intro` (**1 DH**, `es`) and inspects the claimed static:
   - claimed ∈ *known statics* (established connections ∪ pending outbound
     remotes) → **internal continuation** (B3). The packet never touches the
     accept queue and the app never sees it.
   - claimed ∉ known statics → discard the mid-state and **demote** the raw
     packet to the normal stage-0 queue (so a genuinely new peer that happens
     to share a source address with an existing connection — NAT rebind reuse
     — still surfaces as an `Intro`).
4. **`read_identity()` interception (the backstop):** when the app drives a
   parked `Intro` through `read_identity()` and the claimed static turns out
   to ∈ known statics, the endpoint performs the same internal continuation
   and `read_identity()` returns `Err(IntroError::Internal)` — "this
   initiation belonged to an established connection and was consumed; discard
   your handle". The app learns no identity and makes no decision.

**Why the hint is accurate, and why its one false-negative self-heals.** A
rekeying peer's msg1 leaves the same socket as its data packets. Roaming
already keeps the session endpoint at the peer's last authenticated data
source (SPEC §6), so in steady state the msg1's source **matches** the hint.
The only false negative is a NAT rebind landing exactly between the last data
packet and the msg1. In that race the msg1 parks as an `Intro`; but rekey is
send-triggered, so the peer is simultaneously sending payload on the old
session from the new address (old session seals until the swap, inbound
opening is age-exempt) — the first authenticated data packet roams the
endpoint, the hint map updates (via the connection's `AddressMoved` event,
C3), and the peer's next retransmit (~5 s, a completely fresh initiation)
hits the eager path. Worst case the rekey completes one retransmit interval
late; the backstop budget is 60 s (120 s trigger → 180 s teardown), so an
**idle app never kills its own connections** — requirement (ii) holds without
the app draining anything. The parked stale entry ages out per E.

**Why not the alternatives.**

- *WireGuard's eager stage-1 on every mac1-valid msg1* ([wg] §1) conflicts
  with two FIXED constraints: either the endpoint parks the post-`es`
  mid-state (live key material, ~0.5–1 KB holding our static's provider —
  violates stage-0-only parking; [hiss] §2.3.2), or it drops the mid-state
  and the app's later `read_identity()` re-pays `es`, making the accepted
  path 5 DHs — the same pin breakage as the fallback. It also surrenders the
  0-DH reject wholesale: every off-path attacker packet would cost 1 DH.
- *Surface everything, short-circuit only at `read_identity()`* fails
  requirement (ii) outright: an idle app's peers die at the backstop.
- The hint scheme conflicts with neither: hinted packets take an internal
  fast path that is **not an accept at all** (session replacement under an
  existing connection, already ratified behaviour), and unhinted packets take
  the ratified staged path byte-for-byte.

**Supersedes**: nothing ratified — it resolves [actor] hazard 5.2's open
question. The TODO §3 stage table is unchanged for the app-visible path.

### B3. The internal continuation is v1's responder tail, verbatim

**Ruling.** The internal continuation runs exactly v1 SPEC §5 Responder 4–7 on
the already-paid mid-state: `complete()` (`ss`, +1 DH; a forged claim of a
known static dies here at the tail AEAD tag — proof of possession, [hiss]
§3.4), the **greatest-timestamp guard** (strictly greater per static, admit =
check-and-record), the **initiation pacing gate** (B6), then mint a responder
index (re-draw per G), write msg2 (`ee`,`se`, +2 DH), and install the
replacement session under the matched connection — the silent swap: no
event, no accept, no `Established` re-emission, Leg 2 per-epoch reset +
re-queue per SPEC §9.5. If the matched connection had an in-flight outbound
initiation, it is cancelled (H2). Guard/pacing/tag failures → silent drop
with a trace; the established connection is untouched.

### B4. DoS accounting (per attacker packet, mac1 already paid by us as one keyed hash)

| Packet class | Our cost beyond the mac1 hash |
|---|---|
| mac1-invalid garbage / wrong-key | 0 |
| mac1-valid, src ∉ hint set, app never probes or drops the `Intro` | **0 DH**, one bounded queue slot (~220 B) |
| mac1-valid, src ∉ hint set, app probes identity then drops | 1 DH — app-chosen spend |
| mac1-valid, src spoofed into the hint set, claimed static unknown | 1 DH + demotion to the queue |
| forged claim of a known static (src in hint set or app-probed) | 2 DH (`es`+`ss`), dies at the tail tag — exactly v1's accepted exposure for a forged *listed* claim (SPEC §5 step 4) |
| genuine replacement initiation from a key-holding peer | 4 DH, capped in rate by B6 |

Spoofing into the hint set requires knowing an established connection's
current 4-tuple — an on-path observer or a lucky guess — and buys the
attacker exactly WireGuard's baseline (1 DH per mac1-valid packet, [wg] §1),
which WireGuard itself only mitigates with cookies/mac2, deferred for slither
too. Off-path attackers do strictly better against slither v2 (0 DH) than
against WireGuard. This table goes in the draft.

### B5. Per-peer `ss` precomputation: deferred

**Ruling.** WireGuard's amortisation of `ss` to peer-configuration time ([wg]
§1 — `mix_precomputed_dh`, making a known peer's msg1 cost 1 live DH, not 2)
is **not adopted in phase 1**. hiss has no seam for injecting a precomputed
`ss` (`support::ss` calls `provider.dh` internally), and changing that is a
real hiss surface change, unlike the split read. Record two future options in
the draft's deferred-work note: (a) a hiss `complete_with_precomputed_ss`
variant; (b) a memoising `DhProvider` wrapper keyed on the peer public — 
possible today at the provider seam, but its cache must be populated only for
proven peers and bounded, or it becomes attacker-controlled allocation. The
DH-cost pins would need re-ratification either way.

### B6. Initiation pacing (WireGuard's flood gate) — adopted

**Ruling.** Adopt WireGuard's second post-DH admission gate ([wg] §3): a
**per-established-peer pacing limit on session-replacing initiations** of
`INITIATIONS_PER_SECOND = 50` (one per 20 ms), checked alongside the
timestamp guard — it gates *acceptance*, never cost, exactly as in the
kernel. Scope: the internal continuation only (B3); fresh accepts are already
gated by the app. This caps session-churn thrash from a compromised or buggy
key-holding peer. Droppable without structural damage if the maintainer
prefers a smaller phase 1.

---

## C. Sans-io core interface

### C1. Shape: two cores, str0m-style single `poll_output` each **[MAINTAINER]** (confirms TODO open item 5b)

**Ruling.** Sans-io (TODO 5b) is confirmed over the internal-driver
alternative — noting that the shell still runs a driver task (C6); sans-io is
about where the *logic* lives, and quinn's own shell has drivers too ([quinn]
§2). Two pure state machines, `core::Endpoint` and `core::Connection`, each
with the str0m contract: every mutating call is followed by draining
`poll_output()` until the terminal `Timeout` variant, which doubles as the
next-deadline announcement ([quinn] §3 — the drain-complete sentinel and the
deadline query are the same thing, so a driver cannot forget to drain).

**One deliberate deviation from the [quinn] §4 recommendation**: the
connection→endpoint event channel is **folded into `ConnOutput` as a
`ToEndpoint` variant** rather than exposed as a separate
`poll_endpoint_event()`. The report's carve-out exists because str0m's
`poll_output` is app-facing; slither's is **driver-facing** — the application
only ever touches the shell objects (J), and the driver dispatches every
variant anyway. One drain loop, no second queue to forget.

### C2. Responsibility split: handshakes in the endpoint core, sessions in the connection core

**Ruling.** The endpoint core owns everything handshake- and demux-shaped:
classify + oversize drop, mac1, the stage-0 queue and staged-accept verbs,
the hint map and internal continuation (B), **all initiator pendings**
(initial connect *and* rekey: msg1 building, fresh-initiation retransmits
with jitter, 90 s give-up), index minting and both index tables, the
timestamp state and guard, and the root RNG. The connection core owns exactly
today's `Session` + `Recovery`: seal/open, replay window, roaming, the
keepalive/liveness timers, the payload age gates (rekey trigger + backstop),
and the whole Leg 2 frame layer.

**Rationale.** This is v1's proven structure ([actor] §1) and WireGuard's
device/peer split ([wg] §6). It also resolves two entanglements for free:
- msg1 construction needs the endpoint-global timestamp monotonicity and the
  root RNG (D); putting pendings in connection cores would force a
  per-retransmit event round-trip for every draw.
- **Queued sends before establishment stop being a special mechanism**: a
  `core::Connection` exists from `connect()` in the connecting state, and
  early `send()`s land in `Recovery::to_send` as normal — nothing to flush on
  completion, and `queued_sends_before_establish_flow_reliably`'s behaviour
  is preserved structurally ([actor] §1.12; the v1 `pending.queued`
  side-channel dies).

A rekey round-trip is therefore: connection core hits the age gate on a
payload seal → emits `ToEndpoint::NeedsRekey { remote, remote_static }` →
endpoint starts a pending (ignoring the request if one is in flight) →
completion feeds `EndpointToConn::Install(session)` back → connection swaps
(silent, per-epoch reset, re-queue). Give-up on a rekey pending is silent;
on an initial connect it feeds `HandshakeFailed` (J).

### C3. The concrete surfaces

Types below are normative in shape; the draft may rename fields. `Transmit`
is `{ to: SocketAddr, data: Vec<u8> }` (no ECN/GSO at slither's scale).

```rust
impl core::Endpoint {
    fn new(now: Instant, config: Config, identity: Identity, rng_seed: [u8; 32]) -> Self;

    fn connect(&mut self, now: Instant, remote: SocketAddr, remote_static: PublicKey)
        -> Result<(ConnectionId, core::Connection), ConnectError>;   // builds msg1, arms retransmit

    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Disposition;
    fn handle_timeout(&mut self, now: Instant);                       // idempotent
    fn handle_connection_event(&mut self, id: ConnectionId, ev: ToEndpoint);

    fn poll_output(&mut self) -> EndpointOutput;                      // drain to Timeout

    // staged accept verbs — the shell's Intro/Claimed/Proven handles call these
    fn read_identity(&mut self, intro: IntroId) -> Result<PublicKey, IntroError>;   // 1 DH; Err(Internal) = consumed (B2.4)
    fn authenticate(&mut self, intro: IntroId) -> Result<(PublicKey, Timestamp), AuthError>; // +1 DH; guard admits here
    fn accept(&mut self, now: Instant, intro: IntroId)
        -> Result<(ConnectionId, core::Connection), AcceptError>;     // +2 DH; queues msg2
    fn reject(&mut self, intro: IntroId);                             // silent; frees the slot at any stage
}

enum Disposition { ForConnection(ConnectionId), Done }
// ForConnection: the shell feeds the same datagram to that connection core.
// Done: consumed internally (parked, demoted, internal continuation, or dropped).

enum EndpointOutput {
    Transmit(Transmit),                       // msg1/msg2, retransmits, internal-continuation msg2
    IntroReady(IntroId, SocketAddr),          // stage-0 arrival for the accept queue
    ToConnection(ConnectionId, EndpointToConn),
    Timeout(Option<Instant>),                 // terminal: drained + next endpoint deadline
}

enum EndpointToConn {
    Install { session: EstablishedSession, initial: bool }, // initial=false ⇒ silent swap
    HandshakeFailed(ConnectError),                          // initial connect give-up only
}
```

The staged mid-state (post-`read_identity`) lives **inside the endpoint core**
keyed by `IntroId` — the parked queue itself still holds raw bytes only
(stage-0-only parking preserved); mid-states exist only while the app is
actively between stages, bounded by the intro-queue cap (E), and are
documented as live key material to resolve promptly ([hiss] §2.3.2).

```rust
impl core::Connection {
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
    fn handle_timeout(&mut self, now: Instant);                       // idempotent
    fn handle_endpoint_event(&mut self, now: Instant, ev: EndpointToConn);

    fn send(&mut self, now: Instant, msg: &[u8]) -> Result<(), SendError>;            // reliable (tracked DATA)
    fn send_unreliable(&mut self, now: Instant, msg: &[u8]) -> Result<(), SendError>; // untracked DATA (J2)
    fn close(&mut self, now: Instant);                                // local, silent (no wire signal)

    fn poll_output(&mut self) -> ConnOutput;
    // accessors: remote_static(), remote_address(), session_id(), is_established()
}

enum ConnOutput {
    Transmit(Transmit),
    Event(ConnEvent),                 // Established, Message(Vec<u8>), AddressMoved{from,to}, Closed(ConnectionLost)
    ToEndpoint(ToEndpoint),
    Timeout(Option<Instant>),         // terminal
}

enum ToEndpoint {
    NeedsRekey { remote: SocketAddr, remote_static: PublicKey },
    AddressMoved { to: SocketAddr },  // keeps the hint map + rekey targeting fresh (B2)
    Retired { our_index: u32 },       // teardown: endpoint drops the index route
}
```

Output ordering within one drain preserves generation order (a transmit and
the event it caused come out in that order) — state this; it matters for
tests and logs ([quinn] §3 table).

### C4. Time and timers

- **`now: Instant` flows as an explicit argument** on every mutating call;
  cores never call `Instant::now()` or wall clocks (the initiation timestamp
  is the one wall-clock read, obtained via a clock service injected into the
  endpoint core's config — same seam as v1's `next_timestamp`). `poll_output`
  takes no `now`; deadlines are computed from state.
- **Named-timer table + single min-deadline.** `core::Connection` keeps a
  fixed table — `Keepalive, PersistentKeepalive, Liveness, Loss, Pto` — with
  `Timeout(min)` as the exposed deadline, quinn's `TimerTable` pattern
  verbatim ([quinn] §1). `REKEY_AGE`/`REJECT_AGE` are **not timers**: they
  remain payload-path consults (the 2026/07/17 amendment stands). The
  endpoint core's deadline is the min over its pendings'
  `Retransmit`/`GiveUp` and the parked intros' `Expiry`.
- **`handle_timeout` is idempotent**: each due timer is stopped before its
  logic runs, so spurious or repeated calls no-op ([quinn] §1, §5 — this is
  the busy-loop guard; the Firezone stuck-deadline bug is excluded by
  construction).
- **Equal-deadline priorities restated from v1's tick ordering** ([actor]
  §1.1): give-up beats a same-instant retransmit; per connection, loss
  detection beats PTO and exactly one of the two fires per evaluation
  (RFC 9002 §6.2); keepalive evaluation precedes teardown collection.

### C5. RNG injection

**Ruling.** quinn's pattern verbatim ([quinn] §4): the endpoint core owns one
`ChaCha20Rng` seeded from a constructor `[u8; 32]` (config-supplied for
tests, OS entropy otherwise). Every index, jitter, and (via the forced
increment) timestamp draw comes from it. At connection creation the endpoint
draws a 32-byte **sub-seed** and hands it to the connection core; phase 1
connection cores make no draws, but the sub-seed is drawn *anyway* so that
adding connection-side randomness later cannot perturb the endpoint's draw
order. Determinism: one root seed reproduces the whole system. This changes
v1's single-stream draw *ordering* — no golden test depends on it, and the
timing tests depend only on the jitter distribution ([actor] hazard 5.7).

### C6. Shell: one `!Send` driver task, all I/O through it

**Ruling.** The shell is **one driver task** (spawned with `spawn_local`,
`LocalSet` required — v1's contract) that owns the socket for both receive
and send, and owns both cores. Handles — `Endpoint`, `Intro`/`Claimed`/
`Proven`, `Connection` — are thin channel-backed clients. Connections do
**not** send directly on socket clones (quinn's concurrent-send model,
[quinn] §5 "who owns the socket", is explicitly not adopted): slither's
provider seam means the endpoint core is `!Send` whenever the identity
provider is (hardware statics), and a single-task shell gives one code path
for both cases instead of a `Send`-conditional spawn. Connection cores happen
to be `Send` (sessions are plain key material) — noted for future shells, not
exploited in phase 1. The `Wire` trait seam (real socket / `FlakyWire`)
survives as the driver's I/O boundary.

**Driver lifetime = v1's**: the driver lives while any handle lives; dropping
every handle stops it and every session dies silently ([actor] hazard 5.16,
now stated in the spec rather than implied).

**Receive backpressure** (replaces the unbounded global event stream, [actor]
hazard 5.13). Each connection has a **bounded receive buffer** (default 256
messages, configurable). When it is full, an inbound packet is handled as:
decrypt → replay *check* (not mark) → if fresh: update liveness and roaming
(authenticity is established) → then, only if the buffer has room: replay
**mark**, frame processing, ACK scheduling, delivery. A shed packet is never
replay-marked and never ACKed — because the ACK ranges are built from the
replay window (§9.2), marking a shed packet would acknowledge DATA the app
never received and the sender would clear it (permanent loss). Unmarked +
un-ACKed, the peer's loss detection retransmits the frames on fresh counters
and exactly-once still holds via seq dedup. This requires splitting the
window's admit into check and mark — a pure-core change. The accept queue's
bound is E; the handle→driver command channels are bounded, small, and exert
natural `await` backpressure.

---

## D. Endpoint-global state, ratified — and the timestamp-guard bound

**Ruling.** Four globals live in the endpoint core, none per-connection:

1. **The timestamp guard** (per-remote-static greatest timestamp). Scope
   confirmed per-static by all three WireGuard implementations ([wg] §3, §6);
   pushing it into `Connection` would let a peer replay a msg1 into a second
   attempt ([actor] hazard 5.5). Admission (check **and** record) happens at
   `authenticate()` — post-`ss`, so only key-holders can write entries.
2. **`last_init_timestamp`** — endpoint-global outbound monotonic forcing.
   It must survive across connection generations to the same peer (close +
   reconnect must still emit strictly greater), so per-connection scope is
   wrong; endpoint-global is the simple correct superset ([actor] hazard 5.6).
3. **The index demux** — `index → connection` and `pending-index →
   connection`, with mint-time re-draw across **both** tables (G1; required
   because a pending's msg1 index graduates into the session index on
   completion).
4. **The static → connection map** (B1) plus the derived **hint map**
   (connection → current endpoint address), maintained by `AddressMoved`.

**The guard-bounding rule.** Under user-driven accept the guard map is no
longer bounded by an allow-list ([actor] hazard 5.5). Ruling:

- An entry is **pinned** — never evicted — while a live `Connection`, an
  in-flight outbound pending, or a staged mid-state exists for that static.
- All other entries (orphans: dropped intros that were authenticated, dead
  connections) live in a **bounded LRU, default `TS_GUARD_ORPHAN_CAP =
  1024`**, configurable.
- **The eviction consequence, stated honestly in the draft [MAINTAINER]:**
  evicting an orphan re-admits a replay of that static's last initiation. The
  replayed msg1 is genuine, so it authenticates and can surface as a fresh
  `Intro` (or, post-accept, produce a half-open session whose msg2 answers
  nobody — reaped by liveness in 15 s). This is exactly the exposure
  WireGuard accepts on responder restart ("an initial packet from earlier can
  be replayed, but it could not possibly disrupt any ongoing secure
  sessions", [wg] §3) — and pinning guarantees it can never touch an
  *established* connection's replacement protection. Entry cost is ~45 B, so
  the default cap is ~50 KB.

The guard's WireGuard-shaped home would be a per-known-static record tier
([wg] §6's recommendation); phase 1 keeps it as one map with the
pinned/orphan distinction — same semantics, less machinery. The draft should
name the tier as the natural home if per-peer state grows (pacing counters,
B6, live in the same map's values).

---

## E. Stage-0 queue mechanics

**Rulings.**

- **Bound**: `INTRO_QUEUE_CAP = 1024` parked entries, endpoint-wide,
  configurable in `Config`. Rationale: slither's queue is post-mac1 (a higher
  bar than WireGuard's pre-mac1 4096 ring, [wg] §2); 1024 matches
  wireguard-go's handshake queue; at ~220 B/entry (raw 196 B + addr +
  deadline) the worst case is ~225 KB.
- **Dedup key: the full source `SocketAddr` alone, replace-with-newest**
  (replacement refreshes the deadline). **Supersedes TODO §3's
  `(addr, sender_index)` [MAINTAINER]**: the ratified retransmit rule (SPEC
  §5 Initiator 2 — every retransmit is a completely fresh initiation with a
  **new random index**) means the index never matches across retransmits, so
  including it in the key defeats dedup entirely and lets one source occupy
  many slots. (TODO §3's parenthetical "retransmits keep the index"
  contradicts the ratified spec; the spec wins.) Distinct initiators behind
  one NAT have distinct ports, hence distinct keys; a same-4-tuple collision
  is a rebind of the same flow, for which newest-wins is correct — and it is
  the kernel's exact in-place-replace shape ([wg] §3). Note honestly: a
  src-spoofing attacker with valid mac1 can fill distinct slots; that is
  WireGuard-equivalent exposure whose answer is the deferred cookies/mac2
  round.
- **Rekeys and the dedup key**: after B, replacement initiations bypass the
  queue (eager path) or are consumed at `read_identity` — the dedup key no
  longer needs to catch them, which is what made `(addr, sender_index)` look
  necessary ([actor] hazard 5.2).
- **Overflow**: the **incoming packet is silently dropped** (the queue is
  untouched) — the kernel's ring behaviour; the initiator retransmits in
  ~5 s.
- **Deadline**: a parked entry (and its staged descendants' validity) expires
  **`INTRO_TTL = 90 s` after its last refresh** — the initiator's give-up
  horizon (TODO §3). Expiry is silent eviction; staged verbs on an expired
  `IntroId` return `IntroError::Expired`. Practical note for the draft: an
  entry is normally superseded every ~5.3 s by the retransmit, and a msg2
  answering an initiation the peer has since superseded is *ignored by the
  initiator* (completion requires an index match, and each retransmit swaps
  the index), leaving at worst a half-open responder session reaped by
  liveness at 15 s — v1's exact exposure. Hence the spec SHOULD-level advice:
  resolve stages promptly; the queue's replace-with-newest keeps the app
  acting on the freshest attempt.

---

## F. Timer restatement (SPEC §9.4's tick sentence)

**Ruling.** The draft replaces "Both timers are evaluated on the actor's
250 ms TICK" with an implementation-agnostic contract:

> Every armed deadline `D` fires no earlier than `D` and no later than
> `D + L`, where the **lateness bound `L = 250 ms`** is a conformance
> parameter of the shell, not of the protocol. The core exposes exact
> deadlines (`Timeout(Instant)`); a shell may batch or tick provided it
> honours `L`. `K_GRANULARITY` (1 ms) is unchanged; §9.6's "(the 250 ms TICK
> bounds it in practice)" note is rewritten to cite `L`.

**Flow-test tolerances**: existing windows were derived as
`[D, D + tick + jitter-slack]` and remain valid for any implementation at
least as prompt — keep `[10 s, 11.5 s]` (two-loss establish), `[10, 10.6]`
(keepalive), `[15, 15.6]` (liveness), `[90, 90.5]` (give-up), `[120, 180)`
(rekey trigger window) as-is. The one exception:
`asymmetric_loss_retransmit_gate_rekeys_then_backstops`'s 205 s upper bound
encodes the tick-aligned PTO consult schedule ([actor] hazard 5.12); with
exact timers the bound becomes `180 s + (the PTO interval in force at
180 s)`, re-derived from the doubling series at test-writing time — the
draft states the mechanism, not the number.

---

## G. Session-index collision, swap grace, epoch-jump death

**G1. Collision at mint time.** Index minting (session and pending alike)
draws a random nonzero `u32` and **re-draws while the value is present in
either index table**. This closes [actor] hazard 5.3 (v1's unchecked insert
silently steals another connection's Data routing). The nonzero rule and the
uniqueness rule are both protocol; the RNG is the injected root (C5).

**G2. The swap cuts the old session instantly — ratified [MAINTAINER].**
v1's behaviour (old index and keys dropped the moment the replacement
installs; in-flight old-session packets die; [actor] hazard 5.9) is ratified
for v2 and now *stated* in the spec, diverging knowingly from WireGuard's
previous-keypair retention. Rationale: Leg 2 makes the cut safe — undelivered
messages re-queue onto the fresh session (SPEC §9.5), the seq space and
receiver dedup survive the swap so nothing is lost or duplicated, and the
only unrecoverable casualties are keepalives (expendable). A grace window
would buy latency, at the price of two live receive sessions, ACK-source
ambiguity (the old counter space owes no ACK after `epoch_reset` — the
grace-received DATA could be delivered but never acknowledged, forcing the
peer to retransmit it anyway), and roaming/liveness attribution questions.
Revisit with streams if the retransmit cost shows up in practice.

**G3. `MAX_EPOCH_JUMP = 2` is a death condition, enforced by liveness.** A
peer more than two epochs (131 072 counters) ahead is permanently unopenable
([hiss] §4.2) — the receive side refuses without deriving keys. Ruling: **no
dedicated detection or recovery path exists or may be added**. A legitimate
peer can only get there across ≥65 536 messages of silence, which
`DEAD_TIMEOUT` (15 s) excludes by orders of magnitude; if the condition
somehow arises, no inbound packet opens, `last_recv` stops advancing, and the
liveness timer tears the session down — the correct outcome via the existing
mechanism. The draft states this as: "the epoch ratchet's far-future refusal
is subsumed by liveness; implementations must not chase epochs."

---

## H. The two latent bugs — excluded by v2's contracts

**H1. Seal failure can no longer strand frames** ([actor] hazard 5.10: v1's
`pump` mutates recovery state before sealing; a seal error leaves frames
removed from `to_send`, un-tracked, with no timer armed — a permanently
stalled connection). **Ruling: packetisation is plan-then-commit.** The
connection core builds a packet plan, seals, and **only on seal success**
commits the state transition (remove from `to_send`, mark transmitted, clear
`ack_pending`, `on_packet_sent`, arm timers). On seal failure the recovery
state is untouched by construction. Additionally, a seal failure is never
silent: it is only reachable via nonce exhaustion (the reserved `2⁶⁴−1`), and
it moves the connection to `Closed(ConnectionLost::NonceExhausted)` rather
than stalling. The v1 counter-recovery-by-reparse artefact dies with this:
seal returns `(counter, bytes)` (or hiss's `next_counter()`, per A).

**H2. The inbound-msg1-vs-pending-connect race can no longer emit a spurious
`Failed`** ([actor] hazard 5.8: v1 installs the inbound session but leaves
our pending alive; at 90 s it emits `Failed` for an established connection).
**Ruling:** when the internal continuation (B3) installs a session for a
connection that has an in-flight outbound initiation, the endpoint
**cancels that pending** (drops it and its index; no give-up, no error) —
the simultaneous-open resolves to whichever handshake completes first.
Queued sends are unaffected: they live in `Recovery::to_send` (C2) and pump
on whichever session installs. Symmetrically, connect-resolution is
**edge-triggered exactly once** per connection lifecycle: the `Connecting`
future resolves on the first `Install { initial: true }` or
`HandshakeFailed`; rekey installs (`initial: false`) resolve nothing and
re-emit nothing (this also excludes v1's re-emitted `Established` when a
rekey completes after the old session died).

---

## I. Replay window stays at 128

**Ruling.** `REPLAY_WINDOW = 128` is kept for 0.2.0. The wire coupling
forces it: `MAX_ACK_RANGES = 63` is *derived* from the 128-bit window (the
worst-case alternating pattern), a maximal ACK is 268 B, and a v1 peer
**rejects an ACK with more than 63 ranges as a protocol violation** (SPEC
§9.2/§9.6, pinned by `over_cap_ack_is_rejected`). Widening the window to
boringtun's 1024 would make the worst-case ACK 511 ranges ≈ 2 KB —
unrepresentable in one frame under `MAX_PLAINTEXT` and fatal to any 0.1.0
peer — so widening is a wire revision, excluded by the frozen-bytes
invariant. The draft notes the divergence from the references (kernel 8128,
boringtun 1024; [wg] §5) and its real cost honestly: reordering beyond 128
packets drops the stragglers at the window, which Leg 2 converts into
spurious retransmissions, not loss. Revisit in the streams/CC round, where a
new ACK format can ride the reserved frame space.

---

## J. Object API surface

### J1. Staged types — names final **[MAINTAINER]** (closes the TODO naming open item)

`Intro` → `Claimed` → `Proven` → `Connection`, as working-named; ruled final.
They are honest about the security state at each stage in a way quinn's
`Incoming` is not, and `Claimed`'s accessor is `claimed_static()` (never
`remote_static()`), preserving the claimed-vs-proven signal the hiss closure
docs carry today ([hiss] §2.3.3). Shell surface:

```rust
impl Endpoint {
    pub fn builder() -> EndpointBuilder;                     // identity, socket/Wire, Config
    pub async fn accept(&self) -> Option<Intro>;             // None = endpoint closed
    pub fn connect(&self, remote: SocketAddr, remote_static: PublicKey)
        -> Result<Connecting, ConnectError>;                 // Connecting: Future<Output = Result<Connection, ConnectError>>
}

impl Intro {                                                 // 0 DH so far
    pub fn source(&self) -> SocketAddr;
    pub fn sender_index(&self) -> u32;
    pub async fn read_identity(self) -> Result<Claimed, IntroError>;   // 1 DH
}   // drop = silent reject, nothing transmitted

impl Claimed {                                               // 1 DH; identity CLAIMED
    pub fn claimed_static(&self) -> &PublicKey;
    pub async fn authenticate(self) -> Result<Proven, AuthError>;      // +1 DH; guard is not policy
}   // drop = silent reject

impl Proven {                                                // 2 DH; possession proven
    pub fn peer_static(&self) -> &PublicKey;
    pub fn timestamp(&self) -> Timestamp;
    pub async fn accept(self) -> Result<Connection, AcceptError>;      // +2 DH; msg2 sent
}   // drop = silent reject
```

`IntroError::{Expired, Superseded, Internal, Malformed}` — `Internal` is the
B2.4 interception ("consumed by the endpoint; discard"), `Superseded` means a
newer initiation replaced this entry, `Malformed` covers a msg1 whose read
fails structurally. `AuthError::{Replay, HandshakeFailed, Expired}` —
`Replay` is the automatic guard failure. Each stage's async is a driver
round-trip; costs land on the driver task.

### J2. `Connection` (Leg 1 semantics) and the `Reliable` boundary

**Ruling on the layering question the TODO diagram leaves open**: the frame
layer (Leg 2) **always runs in the connection core** — it must (ACK ranges
come from the Leg 1 replay window, control seals are liveness-neutral,
PTO/loss need counters; none of that is buildable outside the core on the
frozen wire). `Reliable<Connection>` is therefore an **API selector, not a
protocol layer**: it exposes the tracked-DATA path, while the base
`Connection` exposes the untracked one.

- `Connection::send_unreliable(&[u8])` — a DATA frame with a fresh seq that
  is **never tracked for loss** (fire-and-forget, at-most-once; the frozen
  wire has no other non-frame plaintext — a raw unframed payload would be a
  protocol violation at the peer). The receiver cannot and need not
  distinguish; dedup and exactly-once surfacing apply as normal.
- `Connection::recv() -> Result<Vec<u8>, ConnectionLost>` — messages surface
  on the base connection regardless of which path the peer used (receive
  semantics are identical by construction, so `Reliable` adds nothing on
  recv; the draft says so explicitly).
- `Reliable::send(&[u8])` — the tracked path: queued, retransmitted, ACKed,
  exactly-once within the connection's life (SPEC §9.3 unchanged).
  `Reliable::new(conn)` / `into_inner()`; recv forwards to the base.
- Accessors: `remote_static()`, `remote_address()` (reflects roaming — the
  `EndpointMoved` event dies; observability moves to the accessor plus the
  `slither::roam` trace target), `session_id()`, `close()` (local silent
  close, unchanged: 0x04 stays reserved and never emitted),
  `set_persistent_keepalive(Option<Duration>)` — **per-connection now**
  (default off), superseding v1's endpoint-wide config knob ([actor] hazard
  5.14).

### J3. Error taxonomy — what replaces the `Event` stream

The global `Event` stream, `ConnId`, `SessionHandle`, `allow`/`revoke`, and
all five events dissolve:

| v1 | v2 |
|---|---|
| `Established` | `Connecting` resolving / `Proven::accept()` returning — exactly once (H2) |
| `Failed { TimedOut }` | `Connecting` resolves `Err(ConnectError::TimedOut)` |
| `Incoming { payload }` | `Connection::recv().await` |
| `Dead` | `recv()`/`send()` return `Err(ConnectionLost::…)` |
| `EndpointMoved` | `remote_address()` accessor + `slither::roam` trace |
| `allow`/`revoke` + allow-list | dissolved: policy = the app's `Claimed` decision; "revoke" = drop the `Connection` |

`ConnectionLost::{TimedOut, RekeyFailed, NonceExhausted, LocallyClosed,
EndpointDropped}` — `TimedOut` is liveness (15 s), `RekeyFailed` is the
180 s payload backstop. `SendError::{PayloadTooLarge, ConnectionLost(…)}`.
The `MAX_MESSAGE` gate stays at the handle, before any channel.

---

## K. Draft scope and structure — confirmed

The draft covers **phase 1 only** (object model + staged accept + sans-io
core), as `SPEC-v2-DRAFT.md`, with **DRAFT** markers everywhere v1 used
RATIFIED. Wire sections are incorporated by reference, never restated (a
restatement can drift). Skeleton for the writer:

1. **Status and scope** — DRAFT banner; phase 1; golden-wire invariant; how
   this document layers over SPEC.md v1.
2. **Unchanged by reference** — §§1–4 in full; §5's prologue/payload/wire
   layout; §6's replay-window shape and the key ratchet; §9.1–§9.3 layouts;
   §9.6 wire constants (`MAX_ACK_RANGES` included). One table listing them,
   citing v1 section numbers.
3. **The object model** *(new)* — `Endpoint`/`Connection`/`Reliable` diagram;
   one-connection-per-static (B1); driver-and-handle lifetimes (C6); drop
   semantics.
4. **Staged accept** *(amends §5 Responder + Allow-list)* — the typestate
   table with DH costs; drop-is-silent-reject; the stage-0 queue (E:
   cap/dedup/overflow/TTL); `IntroError`/`AuthError`; guard admission at
   `authenticate`.
5. **Initiation routing and rekey** *(new; amends §5 Responder 7)* — the
   hint rule, eager path, interception, internal continuation (B2/B3);
   pacing (B6); simultaneous-open cancellation (H2); the DoS table (B4).
6. **The sans-io core** *(new)* — the two cores, the poll contract, the
   method surfaces (C3), `now` flow, named timers + lateness bound `L`
   (C4/F), RNG (C5), plan-then-commit sealing (H1).
7. **Endpoint-global state** *(new)* — the four globals (D); guard
   pinning/LRU and the eviction consequence; index re-draw (G1).
8. **Session behaviour amendments** *(amends §6)* — instant swap cut (G2);
   epoch-death-via-liveness (G3); replay window kept at 128 with the
   derivation chain (I); per-connection persistent keepalive; receive
   backpressure (C6); timer table unchanged in values, restated under `L`.
9. **Leg 2 amendments** *(amends §9.4/§9.5/§9.7)* — the tick sentence
   replaced (F); `send`/`send_unreliable`/`Reliable` semantics (J2);
   queued-sends-through-recovery (C2).
10. **Error taxonomy** *(replaces §7's `Failed` bullet and every event
    mention)* — the J3 table.
11. **Out of scope (phase 1)** — phases 2–4 unchanged; cookies/mac2, `ss`
    precomputation (B5), window widening (I), swap grace (G2) named as
    deferred with one-line pointers.
12. **Appendix: hiss dependencies** — the split read (shape per A) and
    `next_counter()`; non-normative.
13. **Appendix: test migration** — which pins survive verbatim ([actor]
    §4.1 — all of them), which flow tests rewrite harness-only, which
    rewrite as policy ([actor] §4.2), the F windows.

---

## Could not decide — round-2 research tasks

1. **The peer-restart seq-collision.** Replace-in-place keeps our receiver
   dedup floor across the swap (SPEC §9.5), so a peer that *restarts* (rather
   than rekeys) re-sends seqs from 0 and we silently swallow them as
   duplicates — a v1 behaviour v2 preserves, and rekey-vs-restart is
   cryptographically indistinguishable at msg1 time. Task: characterise the
   failure concretely in v1 (test: establish, restart one endpoint's process
   state, reconnect, send), then decide whether replacement should reset
   connection-level Leg 2 state on some signal (e.g. a timestamp gap
   heuristic is unsound; an explicit epoch in a future frame is sound but
   wire work). Likely lands in the streams round; needs a maintainer call on
   whether phase 1 documents it as a known limit.
2. **The `__MSG1_TAIL_SIZE` const derivation in hiss-macros.** A's option (a)
   needs the prefix-tokens `WireSize` computation to be expressible under the
   macro's no-size-arithmetic rule ([hiss] §2.2). Task: spike the const in
   hiss-macros; if it is fiddly, fall back to option (c)
   (`complete(self, &msg1)`) and record the footgun note.
3. **The exact asymmetric-loss test window** (F): compute the PTO consult
   series under exact timers against the phase-1 implementation and pin the
   new upper bound.
4. **hiss error granularity for the far-future epoch refusal** (G3): is the
   refusal distinguishable from a generic decrypt failure, for the trace
   message only? Read `datagram.rs`'s error paths.
5. **Receive-buffer default** (C6): 256 messages is a placeholder; validate
   against the `acks_keep_flow_over_a_lossy_wire` throughput profile.
6. **`TS_GUARD_ORPHAN_CAP = 1024` and `INTRO_QUEUE_CAP = 1024`** are
   defensible defaults, not measurements; sanity-check memory and behaviour
   under a spoofed-src mac1-valid flood in the phase-1 test rig.

## For the spec writer — behaviours the draft must not lose

From the inventory, each pinned or ruled, easy to drop by accident:

- msg2's source address is **deliberately ignored**; the initiator anchors at
  the dialled address and the peer roams in on first authenticated data
  ([actor] 5.15 — now stated, not implied).
- Handshake packets never roam a session; only authenticated fresh Data does.
- One completion attempt per retransmit interval (the `attempt` take): a
  second msg2 in the same interval is dropped.
- Every retransmit is a completely fresh initiation — new ephemeral, new
  index, new strictly-greater timestamp (and E's dedup key depends on it).
- Keepalives are admitted to the replay window but bypass the frame layer:
  they appear in ACK ranges with `ack_delay = 0` and never reach recovery.
- The liveness-neutral seal set (`seal_quiet`): pure ACKs, PTO probes,
  retransmissions. Only fresh application sends and the keepalive mark
  `last_send`. Inbound opening is age-exempt; quiet control is age-exempt.
- Rekey give-up is silent; the old session keeps working. Initial-connect
  give-up is the only handshake failure the app sees.
- An empty `send` is a real (empty) message; an empty *plaintext* is the
  keepalive — the two must not re-merge.
- `MAX_MESSAGE` (1159) is checked at the handle, before any queue.
- ACK processing intersects the in-flight set (bounded), never materialises
  ranges; an ACK above the highest sealed counter is ignored whole.
- Loss beats PTO at the same instant; exactly one of the two fires per
  evaluation; give-up beats a same-instant retransmit.
- Undelivered messages die with the connection (`Dead`, `close`, drop) but
  survive a rekey (re-queue on the fresh session).
- Trace targets `slither::policy`, `slither::replay`, `slither::frames` (plus
  the new `slither::roam`) are operator-visible contract; keep them.
- The golden-wire, DH-cost, size, mac1, frame-layout, replay-window, and
  recovery unit tests all survive unchanged ([actor] §4.1); only harnesses
  rewrite. The allow-list flow tests rewrite as staged-policy tests: "drop
  the `Intro`/`Claimed` ⇒ nothing transmitted, 0/1 DH".

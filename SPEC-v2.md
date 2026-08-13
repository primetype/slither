# slither — protocol specification v2, phase 1

> **DRAFT 2026/08/13 (unratified; round-3 revision).** This document is the
> phase 1 draft of the slither v2 specification: the object model, the staged
> accept, and the sans-io core. It **layers over `SPEC.md` v1** (RATIFIED
> 2026/07/16 and 2026/07/17): v1's wire sections are incorporated by reference
> (§2) and never restated here — a restatement can drift. **No wire byte moves
> in phase 1**: the golden-wire test
> (`golden_wire_is_byte_identical_to_the_pre_migration_driver`) is the
> invariant and must stay green throughout. Every section below carries DRAFT
> where v1 carried RATIFIED; on ratification the markers flip and the code
> must match this file — a later change to either is a protocol revision, not
> an edit. Clauses marked **[MAINTAINER]** need maintainer sign-off before
> ratification. This revision applies the round-2 review resolutions and the
> round-3 fixes (2026/08/13); the changes are indexed against the resolution
> groups and fixes at the end of the document. British English, Oxford comma,
> dates `YYYY/MM/DD`.

## 1. Status and scope *(DRAFT 2026/08/13)*

Phase 1 of the v0.2 round replaces the actor API with a quinn-shaped object
model (`Endpoint`, the staged `Intro → Claimed → Proven → Connection`
typestate, and the `Reliable` wrapper), moves the protocol logic into two
sans-io cores driven by a thin shell, and retires the global `Event` stream,
`ConnId`, `SessionHandle`, and the allow-list. Everything byte-visible is
untouched: v1 §§1–4, the msg1/msg2 layouts and payload, the key ratchet, and
the Leg 2 frame layouts are the ratified wire, and the default channel stays
byte-compatible with 0.1.0 throughout.

Phase 1 gates on a hiss 0.3.x minor shipping the native split msg1 read and
`DatagramSend::next_counter()` (Appendix A). **There is no fallback path in
this specification or in the code** **[MAINTAINER]**: the fallback re-read
would break the ratified DH-cost pins (an accepted msg1 read would cost three
DHs, not two) and perturb the golden vectors via an extra provider draw —
complexity spent on a path intended for deletion.

Suite genericity (phase 2), the `Ordered` wrapper and `split()` (phase 3),
and streams, congestion control, and cookies/mac2 (phase 4) are out of scope
here (§11).

## 2. Unchanged by reference *(DRAFT 2026/08/13)*

The following v1 sections are part of this specification verbatim, by
reference. Where a v1 clause is amended, the amending section of this draft
is named; everything not named is untouched.

| v1 § | Content carried | Status under this draft |
|---|---|---|
| §1 | Crypto suite (`Noise_IK_P256_ChaChaPoly_BLAKE2b`, encodings, datagram transport) | in full |
| §2 | Packet types, version, big-endian rule, reserved types, silent drops | in full |
| §3 | Wire layouts and every size/cap (196/107/14-byte header/`MAX_DATAGRAM` 1200/`MAX_PLAINTEXT` 1170, nonzero-`u32` index, empty plaintext = keepalive, oversize rules) | in full |
| §4 | mac1 — key derivation, label, verify-before-any-DH, not-a-secret-authenticator | in full |
| §5 | Prologue, the encrypted 12-byte initiation timestamp, its confidentiality analysis, msg1/msg2 construction, the strictly-greater forcing | layout and payload in full; the Initiator/Responder step lists are amended by §§4–5 (the staged accept, the routing rule, the post-`ss` simultaneous-open tie-break, and the completion-attempt preconditions); the step-4 membership-oracle acceptance is restated with its v2 population by §5; the Allow-list paragraph is **replaced** by §4 |
| §6 | Replay window shape (RFC 6479, post-AEAD check, drop-without-delivery), roaming rule, timer table **values**, the 2026/07/17 payload-age amendment, the key ratchet (`REKEY_EPOCH_MSGS`, `MAX_EPOCH_JUMP`, straggler tolerance, no-healing) | window shape in full, width ratified unchanged by §8; timer *evaluation* restated by §6/§9; the roaming rule is carried **except as amended by §8**: observability (accessor + trace, not an event) and the receive-backpressure shed rule (a shed packet is window-marked but never delivered or acknowledged); the rekey bullet is amended by §8 (the swap cuts the old session instantly) and the `MAX_EPOCH_JUMP` bullet augmented by §8 (epoch death subsumed by liveness) |
| §7 | Deviations: timestamp-in-payload (resolved), either-side-may-rekey | in full; the `Failed` fifth-event bullet is **replaced** by §10 |
| §8 | Out of scope (Leg 1) | superseded by draft §11 (the phase 1 out-of-scope list); carried as history, not as normative scope |
| §9–§9.3 | Frame model (counter **is** the packet number; frames retransmitted, never packets), frame types and parse rules, ACK layout, descending-range semantics, immediate-ACK policy, ACK-from-the-replay-window, **bounded (intersecting) ACK processing**, DATA layout, seq identity and exactly-once dedup, unordered reliable delivery | in full, with two carve-outs: §9.2's `ack_delay` is measured on the shell-supplied `now` under the §6 lateness bound, not on tokio's clock; and ACK-from-the-replay-window is amended by §8 — every ACK field, `largest` included, derives from the masked set: the window snapshot **AND NOT `shed_mask`** |
| §9.4 | RFC 9002 loss detection and PTO, RTT rules, **an ACK above the highest sealed counter is ignored whole** | in full **except** the final TICK sentence, replaced by §9 |
| §9.5 | Liveness-neutral control (`seal_quiet`), what survives and resets across a swap | in full (restated in §9 to survive the event-language rewrite) |
| §9.6 | Every constant, `MAX_ACK_RANGES = 63` included | in full; the `K_GRANULARITY` parenthetical is rewritten by §9 |
| §9.7 | Behavioural deltas (reliable `send`, the moved message cap, empty-send-is-a-real-message) | in full; its `Incoming` event language is replaced by §10, and the rules are restated in v2 terms by §9 |
| §10 | Out of scope (Leg 2) | superseded by draft §11; carried as history, not as normative scope |

## 3. The object model *(DRAFT 2026/08/13)*

```text
Endpoint                                   // socket + demux; owns the accept queue
├── connect(addr, static) → Connecting     // Future → Connection
├── accept().await → Intro → Claimed → Proven → Connection   (§4)
└── Connection                             // Leg 1+2 as an object, one per remote static
      └── Reliable<Connection>             // API selector for the tracked-DATA path (§9)
```

### One connection per remote static **[MAINTAINER]**

The v1 invariant is ratified for v2: **one logical connection per remote
static, endpoint-wide**. `Endpoint::connect()` to a static that already has a
live `Connection` or an in-flight outbound connect returns
`ConnectError::AlreadyConnected`. An authenticated inbound initiation whose
proven static matches an existing connection is, by definition, a **session
replacement under that connection** (v1 §5 Responder 7) — never a new accept,
never an `Intro` the application decides on (§5). Every routing rule in this
draft keys on this invariant; without it, "which connection does this rekey
belong to" has no answer, and the per-static timestamp guard stops mapping
onto connections. WireGuard's model is identical (one `wg_peer` per static).
This forecloses multi-connection-per-peer for the 0.2 line. `connect()` to
our own static is out of scope under this rule, which is what makes the
simultaneous-open tie-break's equal-statics case unrepresentable (§5).

### Shell surface

```rust
impl Endpoint {
    pub fn builder() -> EndpointBuilder;                     // identity, socket/Wire, Config
    pub async fn accept(&self) -> Option<Intro>;             // None = endpoint closed
    pub fn connect(&self, remote: SocketAddr, remote_static: PublicKey)
        -> Result<Connecting, ConnectError>;                 // Connecting: Future<Output = Result<Connection, ConnectError>>
}
```

`connect` is not gated by any policy — the caller chose the target (v1 §5,
unchanged in spirit). `Connecting` resolves exactly once per connection
lifecycle (§5, simultaneous open).

### Driver and handle lifetimes

The shell is **one `!Send` driver task** (spawned with `spawn_local`; a
`LocalSet` is required — v1's contract), which owns the socket for both
receive and send and owns both sans-io cores (§6). `Endpoint`,
`Intro`/`Claimed`/`Proven`, and `Connection` handles are thin channel-backed
clients; connections do **not** send on socket clones. Rationale: the
provider seam makes the endpoint core `!Send` whenever the identity provider
is (hardware statics), and a single-task shell gives one code path for both
cases. Connection cores happen to be `Send` (sessions are plain key
material) — noted for future shells, not exploited in phase 1. The `Wire`
trait seam (real socket or `FlakyWire`) is the driver's I/O boundary.

**The driver lives while any handle lives.** Dropping every handle stops the
driver, and **every session dies silently with it** — no teardown, nothing
transmitted. This is v1's behaviour, now stated rather than implied.

### Drop semantics

Dropping a staged object (`Intro`, `Claimed`, `Proven`) is a **silent
reject**: nothing is transmitted, the queue slot is freed (§4). Dropping the
last handle to a `Connection` is `close()`: a local, silent teardown — the
`0x04` close packet stays reserved and is never emitted (v1 §2), undelivered
messages are lost (v1 §9.3). "Revoke" has no verb in v2: policy is the
application's staged decision on the way in, and dropping the `Connection` on
the way out.

## 4. Staged accept *(DRAFT 2026/08/13 — amends v1 §5 Responder and replaces the Allow-list)*

The v1 allow-list closure is turned inside out: the responder's staged DH
costs (v1 §5 Responder 2–4) become an application-driven typestate. The type
names `Intro`, `Claimed`, and `Proven` are final **[MAINTAINER]** — each is
honest about the security state it represents.

### The typestate and its DH costs

| Stage | Cumulative responder cost | Visible to the application | Automatic (non-policy) rejections |
|---|---|---|---|
| `Intro` — length-gated, classified, mac1-verified, parked | 1 keyed hash, **0 DH** | source address, `sender_index` | short/oversize, unknown type/version, bad mac1 — all silent, before the queue |
| `read_identity()` → `Claimed` | **1 DH** (`es`)† | the **claimed** static | structurally unreadable msg1 (`Malformed`) |
| `authenticate()` → `Proven` | **2 DH** (+ `ss`) | possession proven, initiation timestamp | tail-tag failure (`HandshakeFailed`); timestamp replay (`Replay` — **the guard is not policy**) |
| `accept()` → `Connection` | **4 DH** (+ `ee`, `se`; msg2 sent) | an established connection | — |

† A hinted-source entry may arrive with its identity **pre-read** (§5 step
3): the 1 DH was charged once, at the eager read, and the application's
`read_identity()` on such an entry returns the cached claimed static at **0
incremental DH**. The cumulative table is unchanged either way — Claimed 1 /
Proven 2 / Connection 4.

The table prices `accept()`'s fast path (the chain's own initiation still
the freshest). A **re-homed** `accept()` — the freshest-parked-initiation
rule below — adds that initiation's `es` + `ss` on top of the 4: an
application-driven spend, priced in §5's cost notes.

Dropping the object at any stage is a **silent reject**: no msg2, nothing
transmitted, the slot freed. The claimed static at `Claimed` is
attacker-choosable (reaching it requires no secret — v1 §5 Responder 2's
"claim is not yet proof" contract), and the same discipline applies one stage
earlier: `source()` and `sender_index()` are exposed at 0 DH and are equally
attacker-chosen (the source may be spoofed; the index is cleartext,
attacker-supplied). **Nothing durable may be keyed on the claimed static, the
source address, or `sender_index`** — no map insertion, no rate-limit bucket,
no unbounded logging. Proof of possession arrives only at `authenticate()`,
where a forged claim of a real static dies at the msg1 tail's AEAD tag (v1 §5
Responder 4). The **greatest-timestamp guard admits (checks and records) at
`authenticate()`** — post-`ss`, so only key-holders can write guard entries
(§7).

```rust
impl Intro {                                                 // 0 DH so far
    pub fn source(&self) -> SocketAddr;
    pub fn sender_index(&self) -> u32;
    pub async fn read_identity(self) -> Result<Claimed, IntroError>;   // 1 DH (0 on a pre-read entry)
}   // drop = silent reject, nothing transmitted

impl Claimed {                                               // 1 DH; identity CLAIMED
    pub fn claimed_static(&self) -> &PublicKey;              // never `remote_static()`
    pub async fn authenticate(self) -> Result<Proven, AuthError>;      // +1 DH; guard admits here
}   // drop = silent reject

impl Proven {                                                // 2 DH; possession proven
    pub fn peer_static(&self) -> &PublicKey;
    pub fn timestamp(&self) -> Timestamp;
    pub async fn accept(self) -> Result<Connection, AcceptError>;      // +2 DH (a re-home adds 2 more); msg2 for the freshest parked initiation
}   // drop = silent reject
```

`Claimed`'s accessor is `claimed_static()`, preserving the claimed-vs-proven
signal that the v1 closure documentation carries. Each stage's `async` is a
driver round-trip; the DH costs land on the driver task.

**Errors.** The staged error types — normative home §10, restated here for
the verbs they serve. `IntroError::{Expired, Internal, Malformed,
EndpointDropped}`: `Expired` — the entry outlived `INTRO_TTL`; `Internal` —
the initiation belonged to a known static and was consumed by the endpoint's
internal continuation (§5); the application learns no identity and makes no
decision — discard the handle; `Malformed` — the msg1 read fails
structurally; `EndpointDropped` — the driver task stopped between request
and reply. `AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}`:
`Replay` is the automatic guard failure; `HandshakeFailed` is the tail-tag
death of a forged claim — the only staged variant that is a security signal
(§10); `Expired` and `EndpointDropped` as above, at this stage.
`AcceptError::{Stale, AlreadyConnected, EndpointDropped}`:
`Stale` — no initiation for the proven static is currently parked (the peer
stopped retrying, or every parked one aged out; the re-home rule below) —
the application SHOULD re-accept when the peer's next initiation surfaces
as a new `Intro`; `AlreadyConnected` — a session for this static installed
meanwhile (§3). **There is no `Superseded` variant in any enum** — see
*Consumption and supersession* below — **and no `AcceptError::Expired`**:
a chain's age never fails an `accept()`; only the absence of any parked
initiation does (the re-home rule).

### The stage-0 queue

| Constant | Value | Notes |
|---|---|---|
| `INTRO_QUEUE_CAP` | **1024** slots, endpoint-wide | configurable in `Config`; default pending the phase-1 flood validation (draft-note, removable at ratification) |
| `INTRO_MAX_PER_SOURCE` | **4** chains per source IP (per /64 for IPv6) — the sum of unconsumed stage-0 entries and consumed chains | configurable; same draft-note |
| `INTRO_TTL` | **15 s** after the entry's last refresh | configurable; same draft-note |

The accept queue parks **stage-0 state only** — the raw ~196-byte msg1 plus
the source address (~220 B per entry; worst case ~225 KB at the default cap)
— with one bounded exception: an eager-demoted entry carries its already-paid
mid-state (§5 step 3), keyed by hint-set sources (≤ connections + pendings)
and counted against the same cap. The bound is post-mac1 — a higher bar than
WireGuard's pre-mac1 4096-slot ring — and 1024 matches wireguard-go's
handshake queue.

The three flood mitigations below — evict-oldest overflow, the per-source
cap, and the 15 s TTL — are a ruled posture **[MAINTAINER]**: each is cheap,
none is load-bearing for correctness, and together they change the flood
exposure from a holdable reservation to a per-packet race (honesty clause
below).

- **Dedup key: the full source `SocketAddr` alone; replace-with-newest**,
  and replacement refreshes the deadline. This **supersedes TODO.md §3's
  `(addr, sender_index)` key [MAINTAINER]**: v1 §5 Initiator 2 ratifies that
  *every retransmit is a completely fresh initiation — new ephemerals, a new
  random index, and a new strictly-greater timestamp* — so an index-bearing
  key would never match across retransmits, defeating dedup and letting one
  source occupy many slots. Distinct initiators behind one NAT present
  distinct ports, hence distinct keys; a same-4-tuple collision is a rebind
  of the same flow, for which newest-wins is correct (the kernel's exact
  in-place-replace shape). Replacement initiations from established peers
  bypass this queue entirely (§5), so the key need not catch them.
- **Per-source cap**: at most `INTRO_MAX_PER_SOURCE` chains per source IP
  (per /64 for IPv6), counting the **sum of unconsumed stage-0 entries and
  consumed chains** — the 0-DH analogue of WireGuard's per-IP token bucket.
  An arrival that would exceed the cap replaces that IP's oldest
  **unconsumed** entry — eviction operates on the unconsumed tier only;
  consumed chains are DH-paid and app-held, freed by the application
  resolving or dropping the handle or by TTL — and if the source's whole
  allowance is held by consumed chains, the arrival is dropped.
  `read_identity()` consuming an entry is net-zero for its source's count
  (−1 unconsumed, +1 consumed), so the cap bounds a source to 4 chains
  total regardless of how the application probes.
- **Overflow: evict-oldest.** When the queue is full, the arriving initiation
  **evicts the oldest unconsumed entry** (by park time) rather than being
  dropped — a genuine initiation always obtains a slot, and an attacker must
  win a per-packet race against the genuine peer's ~5 s retransmit rather
  than hold a permanent reservation. Consumed chains are never evicted by
  overflow.
- **Expiry** is silent eviction at `INTRO_TTL`: the slot is freed and any
  mid-state discarded; staged verbs on an expired attempt return
  `IntroError::Expired` (`AuthError::Expired` at that stage). A parked
  entry's staged descendants share its validity — consumption does not
  extend it (later initiations park separately), so a consumed chain's
  mid-state expires 15 s after the initiation that fed it. `accept()` alone
  is exempt: the re-home rule (below) establishes on the freshest parked
  initiation for the proven static, so a chain's age never fails an
  `accept()` — only the absence of any parked initiation does
  (`AcceptError::Stale`). 90 s (the previous value) was the *initiator's*
  give-up horizon, not the responder's obligation; entries are superseded
  every ~5.3 s in normal operation, so 15 s is ample and cuts the flood
  hold cost ~6×.

### Consumption and supersession — own-bytes-on-consume

- A parked entry is **unconsumed** until the application calls
  `read_identity()` on it. While unconsumed, a newer initiation from the same
  source **transparently replaces** the entry's bytes and refreshes its TTL:
  same `IntroId`, newest bytes. The handle's accessors (`source()`,
  `sender_index()`) reflect the **newest bytes at call time**, and a
  replacement of an already-surfaced entry emits **no second `IntroReady`** —
  surfacings are bounded to one per source. The application has spent nothing
  on an unconsumed entry, so transparent replacement is harmless.
- The moment `read_identity()` is called, **the chain owns its bytes and its
  `IntroId`**: the stage-0 dedup slot for that source is freed, and a
  subsequent initiation from the same source parks as a **new** stage-0 entry
  (new `IntroId`, new `IntroReady`). A consumed `Claimed`/`Proven` chain can
  therefore **never** be superseded by any later packet — an unauthenticated
  mac1-valid packet cannot clobber DH-paid work, restoring v1 parity (in v1 a
  spoofed garbage msg1 died at the tail tag and could not disturb a
  concurrent genuine handshake, because there was no per-source slot for it
  to clobber).
- **`Superseded` is removed from every error enum [MAINTAINER].** Under
  own-bytes-on-consume no verb can observe supersession: stage-0 replacement
  is transparent (nothing for `read_identity()` to report), and a consumed
  chain is isolated (nothing later can touch it). The variant is unreachable
  and is deleted rather than extended. This knowingly diverges from both
  round-2 reviewers' literal fix (add `Superseded` to
  `AuthError`/`AcceptError`): the own-bytes model removes the attack *and*
  the variant.
- **Accounting.** Accept-side state is one budget of `INTRO_QUEUE_CAP`
  slots. An unconsumed stage-0 entry holds a slot — attacker-reachable at
  0 DH, per-source-capped, TTL-evicted, evict-oldest under overflow. A
  consumed chain keeps holding its slot until the application resolves or
  drops the handle (or the TTL frees its mid-state) — creating one cost the
  application ≥ 1 DH by choice, but a single source can *induce* a probing
  application into that spend, which is why the per-source cap counts
  **consumed and unconsumed chains together** (above). Evict-oldest — the
  per-source replacement and the overflow eviction alike — operates on
  unconsumed entries only. §7's ceilings table prices the composite worst
  case.
- Edge: after consumption frees the source slot, the peer's next retransmit
  parks anew, so the application may briefly hold a `Claimed` (from
  retransmit N) and a fresh `Intro` (retransmit N+1) for the same peer. It
  resolves one; the one-connection-per-static rule (§3) makes the second
  `accept()` return `AcceptError::AlreadyConnected`.
- **Resolve stages promptly (SHOULD).** An unconsumed entry is refreshed
  every ~5.3 s by the retransmit; a consumed chain's `read_identity()` and
  `authenticate()` ride the initiation that fed it (at most 15 s of
  validity). `accept()` is decoupled from it by the re-home rule (below):
  a msg2 answering an initiation the peer has since superseded would be
  ignored by the initiator (completion requires an index match, and each
  retransmit swaps the index), leaving a half-open responder session reaped
  by liveness in 15 s — the failure mode the re-home exists to exclude.

**Mid-states are live key material.** A mid-state — post-`read_identity` on
a consumed chain, or carried by an eager-demoted entry (§5 step 3) — lives
inside the endpoint core keyed by `IntroId`, never in the raw parked bytes.
It holds the endpoint's static provider and the `es`-derived keys
(~0.5–1 KB); it is bounded by the queue cap and the TTL. Resolve or drop
promptly.

**Honesty clause — the queue-occupancy exposure.** mac1's key is public data
(v1 §4), so minting mac1-valid initiations costs an attacker nothing but
bandwidth — and **no spoofing capability is needed to occupy slots**: the
dedup key includes the port, so a single unspoofed host presents distinct
sources from distinct source ports. What the caps buy, and what they do not:
`INTRO_MAX_PER_SOURCE` bounds one source to 4 chains **total — consumed and
unconsumed together** — so filling the queue needs ≥ 256 distinct source
addresses or forged sources, and a single source cannot induce a probing
application into filling the budget with consumed chains (a distributed
attacker still needs one IP per 4 chains, and the application's probe
policy governs how many it authenticates); evict-oldest means a full
queue is never a reservation — a genuine initiation always obtains a slot,
and the attacker must win a per-packet eviction race against the genuine
peer's ~5 s retransmit; and the 15 s TTL prices sustained full occupancy at
≈ 68 packets/second of refresh traffic (1024 slots / 15 s). The denial,
while sustained at that rate, is **endpoint-wide for new inbound accepts** —
not merely a memory cost — though established connections and their rekeys
are untouched (they never enter this queue, §5). This is **not**
WireGuard-equivalent exposure: WireGuard's 4096-slot ring is a transient
work queue drained at line rate, backed by the under-load cookie gate and a
per-IP token bucket; slither holds slots pending an application decision
and, until the deferred cookies/mac2 round (§11), has only the per-source
slot cap in that role.

### `accept()` re-homes to the freshest parked initiation

The staged chain (`Intro` → `Claimed` → `Proven`) proves *identity*;
`accept()` commits to the *peer* — never to the specific initiation the
application inspected. Fresh-ephemeral retransmit means the initiator
re-mints its index every ~5 s and ignores a msg2 answering a superseded
initiation, so a `Proven` chain's own initiation goes stale in about one
retransmit interval — far inside a human-in-the-loop accept decision. The
rule (ruled; maintainer sign-off is tracked at §11's known-behaviour
clause):

- **Fast path.** If no newer initiation from the peer has parked since the
  chain consumed its own — the `Proven`'s initiation is still the
  freshest — `accept()` proceeds on it exactly as the table above prices:
  no re-home, no extra DH. Prompt decisions land here.
- **Re-home.** Otherwise the endpoint replays the parked candidates for the
  chain's source in order of **park time, newest first** — park time, not the
  msg1 timestamp, because the timestamp is inside the encrypted payload and is
  not known until `es` + `ss` have run (candidates are identity-unread parked
  entries from the chain's source; a source-changing rebind surfaces as a new
  `Intro` instead). For each candidate the endpoint runs `es` + `ss`; a
  candidate is **admitted only if all three hold** — the read yields the
  **same** proven static, the tail tag verifies, **and** the timestamp guard
  admits its strictly-greater timestamp — and msg2 is written for that
  initiation. A candidate failing **any** of the three (wrong static, bad tag,
  or a non-greater timestamp — the shape a *replayed* genuine initiation takes)
  is discarded and the next-newest is tried, until one is admitted or the
  candidates for that source are exhausted. Because the peer retransmits every
  ~5 s and `INTRO_TTL` keeps ~3 recent initiations parked, an admissible one is
  essentially always available while the peer is still trying (its full 90 s
  give-up window). The per-source cap (`INTRO_MAX_PER_SOURCE = 4`) bounds the
  candidate list, so a replay/spoof attacker who parks stale genuine
  initiations as the newest entries costs the accepting endpoint at most four
  `es` + `ss` pairs before the genuine fresh retransmit is reached — a bounded
  delay, never a stall.
- **Stale.** If no initiation for that static is currently parked (the peer
  stopped retrying, every parked one aged out, or the application is itself
  holding that source's full `INTRO_MAX_PER_SOURCE` allowance of consumed
  chains, leaving no slot for a fresh initiation to park), `accept()` returns
  `AcceptError::Stale`; the application SHOULD re-accept when the peer's
  next initiation surfaces as a new `Intro`, dropping stale sibling chains it
  no longer needs.

Cost and the deferred richer model are documented in §5's cost notes and
§11's known-behaviour clause.

## 5. Initiation routing and rekey *(DRAFT 2026/08/13 — new; amends v1 §5 Responder 7)*

The load-bearing rule of this draft. Requirements: the application must not
re-approve — ideally not even see — a rekey of an established connection;
rekey processing must not depend on the application draining the accept queue
(an idle application must not kill its own connections at the 180 s
backstop); the 0-DH drop of an unwanted `Intro` survives; the
per-attacker-packet cost is bounded and stated; the 1/2/4 cumulative DH costs
of §4 are preserved on the application-visible path; stage-0-only parking
survives (with §4's one bounded carried-mid-state exception).

### The routing rule

Inbound `HandshakeInit` processing in the endpoint core:

1. **Stage 0 (always):** length gate, classify, mac1 verify. Failure is a
   silent drop. Cost: one keyed hash. Unchanged from v1.
2. **Hint check (no DH):** the *hint set* is the current endpoint addresses
   of all established connections ∪ the dialled addresses of all in-flight
   outbound initiations. If `src` ∉ hint set → **park at stage 0** (§4) and
   surface an `Intro`. This is the staged path of §4, byte-for-byte, 0 DH
   until the application asks.
3. **Eager path (`src` ∈ hint set):** the endpoint immediately runs the
   split intro read (**1 DH**, `es`) and inspects the claimed static:
   - claimed ∈ *known statics* (established connections ∪ pending outbound
     remotes) → the **internal continuation** (below); where an in-flight
     outbound pending exists for that static, the continuation applies the
     **simultaneous-open tie-break** — post-`ss`, on the authenticated
     inbound only (below). The packet never touches the accept queue and
     the application never sees it.
   - claimed ∉ known statics → the raw packet is **demoted** to the stage-0
     queue under the normal §4 rules, **carrying its paid mid-state**,
     tagged identity-already-read: a genuinely new peer that happens to
     share a source address with an existing connection (NAT rebind reuse)
     still surfaces as an `Intro`, and the application's `read_identity()`
     on it returns the **cached** claimed static at **0 incremental DH** —
     the packet's one `es` is charged at the eager read and never re-paid.
     Carried entries are keyed by hint-set sources (≤ connections +
     pendings) and count against the queue cap like any parked entry (§4).
4. **`read_identity()` interception (the backstop):** when the application
   drives a parked `Intro` through `read_identity()` and the claimed static
   turns out to be a known static, the endpoint performs the same internal
   continuation (tie-break included where it applies) and `read_identity()`
   returns `Err(IntroError::Internal)`. The application learns no identity
   and makes no decision.

**Why the hint is accurate, and why its one false negative self-heals.** A
rekeying peer's msg1 leaves the same socket as its data packets, and roaming
keeps the session endpoint at the peer's last authenticated data source (v1
§6), so in steady state the msg1 source matches the hint. The only false
negative is a NAT rebind landing exactly between the last data packet and the
msg1. In that race the msg1 parks as an `Intro`; but rekey is send-triggered,
so the peer is simultaneously sending payload on the old session from the new
address (the old session seals until the swap; inbound opening is
age-exempt) — the first authenticated data packet roams the endpoint, the
hint map updates via the connection's `AddressMoved` event (§6), and the
peer's next retransmit (~5 s, a completely fresh initiation) hits the eager
path. Worst case the rekey completes one retransmit interval late, against a
backstop budget of 60 s (120 s trigger → 180 s teardown) — **an idle
application never kills its own connections**. The stale parked entry ages
out per §4.

**The membership-timing oracle, restated for v2 [MAINTAINER].** v1 §5 step 4
ratified an accepted ~one-ECDH timing difference by which a prober could test
a public key's allow-list membership. v2 changes the oracle's population and
sharpens its interface, so the acceptance is restated rather than silently
inherited: the probed set is now the **live known-static set** (established
connections ∪ pending outbound remotes) — a *liveness* oracle, not a
configuration oracle — and it is exposed both as timing (the extra `ss` of
the continuation) and as an explicit API discriminator: probing an initiation
that claims a known static returns `IntroError::Internal`, while an unknown
claim yields `Claimed` at one DH less. Statics are public data; the exposure
is accepted for v2 as its predecessor was for v1.

### Simultaneous open — the deterministic tie-break

When an inbound initiation's claimed static matches a peer to whom we hold an
in-flight outbound pending — the match is detected the moment the eager
path's `es` yields the claimed static, or at the `read_identity()`
interception; for a dialled peer the static is already held as the dialled
remote — the race is resolved by a tie-break evaluated identically on both
sides, never by completion order (each side's *inbound* handshake always
completes first locally, so "whichever completes first" would have both
sides install different key sets and go mutually dark for 15 s): **the peer
with the lexicographically smaller static public key is the winning
initiator**. The comparison is over the 33-byte compressed encodings (the
mac1-keying bytes, v1 §1/§4), compared as unsigned big-endian octet strings.

**The tie-break runs only on an authenticated inbound — after `ss`
succeeds.** The claimed static in msg1 is forgeable by anyone holding the
responder's public static (only `ss` binds possession), so a match detected
at `es` selects the tie-break path but decides nothing: the continuation
runs `es` → `ss`, and an `ss` failure is a silent drop with **the pending
untouched** — a forgery dies at `ss` and cannot cancel a pending. Only when
`ss` succeeds (and the guard and pacing gates pass — the continuation's
numbered order below) is the tie-break applied:

- **Our static is smaller** ⇒ we are the winning initiator: the
  (authenticated) inbound msg1 is **silently dropped** — its mid-state
  discarded; no msg2, nothing recorded — and our own outbound completes
  normally.
- **The peer's static is smaller** ⇒ we cancel our pending **now,
  post-`ss`, on the authenticated inbound** (the pending and its index are
  dropped — no give-up, no error) and the continuation admits and writes
  msg2 as responder.

Both sides compare the same ordered pair and reach complementary
conclusions, so exactly one session — built from the winner's msg1 and the
loser's msg2 — is constructed, and both sides hold it. Cost: 2 DH
(`es` + `ss`) on the inbound before deciding — the honest simultaneous-open
cost; for a forgery it is exactly the DoS table's
forged-claim-of-a-known-static row (2 DH, dies at the tail tag; no new
row). A connecting
(never-established) connection that loses the tie-break is completed by the
continuation's `Install { initial: true }` (§6), which resolves its
`Connecting` exactly as a msg2 completion would — connect resolution stays
**edge-triggered exactly once** per connection lifecycle: the `Connecting`
future resolves on the first `Install { initial: true }` or
`HandshakeFailed`; rekey installs (`initial: false`) resolve nothing and
re-emit nothing. (This excludes v1's spurious 90 s `Failed` for an
established connection, and its re-emitted `Established` when a rekey
completed after the old session died.) Equal statics cannot occur:
`connect()` to our own static is out of scope under the
one-connection-per-static rule (§3), so a self-connection is
unrepresentable. Queued sends are unaffected by either outcome: they live in
the connection core's send queue (§9) and pump on whichever session
installs.

### The internal continuation is v1's responder tail, verbatim

The continuation runs exactly v1 §5 Responder 4–7 on the already-paid
mid-state, in this order:

1. **Tag** — `complete()` (`ss`, +1 DH): a forged claim of a known static
   dies here at the msg1 tail's AEAD tag — proof of possession. A failure
   leaves any in-flight outbound pending untouched: nothing
   unauthenticated can reach the tie-break.
2. **Guard** — the greatest-timestamp guard: strictly greater per static
   (v1 §5 Responder 5). A replayed genuine msg1 dies here — before it can
   touch a pending.
3. **Pacing** — the per-known-static gate (below).
4. **Tie-break** — only where an in-flight outbound pending exists for the
   now-authenticated static (§5's simultaneous-open rule): our static
   smaller ⇒ the inbound is silently dropped and our pending kept (no msg2,
   nothing recorded); the peer's static smaller ⇒ our pending and its index
   are cancelled (no give-up, no error) and the continuation proceeds.
5. **Admit** — only on passing all of the above is the strictly-greater
   timestamp **recorded** (admission = check-and-record **on full admission
   only**; a pacing or guard failure — or a won tie-break's drop — records
   nothing), a responder index minted (re-drawn per §7), msg2 written
   (`ee`, `se`, +2 DH), and the session installed — a replacement under the
   matched connection, or a lost tie-break's completion via
   `Install { initial: true }` (§6).

A failure at any of steps 1–3 — tag, guard, or pacing — is a **silent drop
with a trace** (`slither::policy`), the established connection (and any
in-flight pending) untouched, and nothing recorded; a step-4 winner-side
drop is likewise silent, traced, and record-free. The install is **the silent swap**: no event, no
accept, no re-resolution of anything, Leg 2 per-epoch reset and re-queue per
v1 §9.5.

**Initiation pacing (adopted from WireGuard's flood gate).**

| Constant | Value |
|---|---|
| `INITIATIONS_PER_SECOND` | **50** — mechanism: minimum **20 ms spacing** between accepted replacements, per known static |

The pacing gate's scope is **per known static** — established connections
and pending outbound remotes alike — with the pacing counter stored in that
static's guard entry (§7), which exists for every continuation trigger: §7
pins entries for established connections and in-flight pendings, and
`connect()` creates the entry for a dialled static. The mechanism is
minimum spacing: an accepted replacement less than 20 ms after the previous
accepted replacement for the same static is rejected at step 3. It gates
*acceptance*, never cost — exactly as in the kernel — and caps
session-churn thrash from a compromised or buggy key-holding peer. Scope:
the internal continuation only; fresh accepts are already gated by the
application. Severability note (carried from the design round): the pacing
gate is structurally severable — deleting it removes step 3 and this table
and touches nothing else — if the maintainer prefers a smaller phase 1.

### Initiator pendings

All initiator pendings — initial connect *and* rekey — live in the endpoint
core (§6). Their behaviour is v1 §5 Initiator 1–4 unchanged: each retransmit
is a completely fresh initiation (new ephemerals, new index, new
strictly-greater timestamp) at `RETRANSMIT_BASE` + jitter; give-up at
`HANDSHAKE_GIVEUP` (90 s). Restated now rather than implied:

- **One completion attempt per retransmit interval**: the pending's attempt
  state is *taken* on the first **length-correct, index-matching,
  mac1-valid** msg2 (v1 §5 Initiator 3's preconditions, restated so that a
  guessed-index or mac1-invalid msg2 can never spend anything); a second
  msg2 in the same interval is dropped. A failed completion spends the
  attempt — hiss's consuming state machines foreclose retrying the read —
  and the **next scheduled retransmit** (at `RETRANSMIT_BASE` + jitter)
  refreshes it: at most one fresh initiation per retransmit interval, no
  matter how many forged msg2s arrive.
- **The msg2 source address is deliberately ignored.** Completion requires
  an index match, not an address match; the initiator anchors the session at
  the **dialled** address, and the peer roams in on its first authenticated
  data packet (§8).
- **Rekey give-up is silent** — the old session keeps working (until
  liveness or the payload backstop rules otherwise). **Initial-connect
  give-up is the only handshake failure the application ever sees**:
  `Connecting` resolves `Err(ConnectError::TimedOut)`.

### DoS accounting

Per attacker packet; the mac1 verification (one keyed hash) is already paid
by us in every row.

| Packet class | Our cost beyond the mac1 hash |
|---|---|
| mac1-invalid garbage / wrong-key | 0 |
| mac1-valid, src ∉ hint set, application leaves the `Intro` unprobed (or drops it unprobed) | **0 DH**, one bounded queue slot (~220 B) |
| mac1-valid, src ∉ hint set, application probes identity then drops | 1 DH — an application-chosen spend |
| mac1-valid, src spoofed into the hint set, claimed static unknown | **1 DH** — the `es` is paid once at the eager read and carried through the demotion; the application's later `read_identity()` is free (step 3) |
| forged claim of a known static (src in hint set, or application-probed) | 2 DH (`es` + `ss`), dies at the tail tag — exactly v1's accepted exposure for a forged *listed* claim (v1 §5 step 4); this row also prices a forged simultaneous-open claim, which dies here with the pending untouched (the tie-break is post-`ss`) |
| replayed genuine msg1 from a hint-set source (recorded once, replayed at line rate) | **2 DH** (`es` + `ss`), dies at the timestamp guard — **not** capped by the pacing gate, which gates acceptance after both DHs; the cheapest sustained 2-DH primitive in the design |
| genuine replacement initiation from a key-holding peer | **2 DH per initiation, uncapped** (`es` + `ss`); + 2 DH (`ee` + `se`) only for the ≤ `INITIATIONS_PER_SECOND` accepted |

**The ceiling, explicitly:** the maximum cost of any single attacker packet
is **2 DH**, and only 1 of those is reachable without either the attacker
holding a hint-set address or the application choosing to spend.

One initiator-side exposure belongs in this accounting: a mac1-valid,
index-matching but cryptographically invalid msg2 **spends the initiator's
completion attempt** for that interval. This is an **on-path
(index-observing) capability** — `sender_index` rides in cleartext, but an
off-path attacker must guess a 32-bit index — and its full mitigation
(retaining enough state to retry a failed read) is foreclosed by hiss's
consuming state machines. The exposure is documented rather than mitigated:
the attempt refreshes at the next scheduled retransmit, so a sustained
on-path forger who beats the genuine msg2 each interval denies the
handshake for as long as it sustains (bounded by the 90 s give-up) — and
can never induce initiations faster than the retransmit schedule.

One accept-side note completes the table: a **re-homed `accept()`** (§4)
pays the inspected chain's `es` + `ss` plus the freshest initiation's
`es` + `ss` + `ee` + `se`. This is app-driven, post-authentication (both
tag checks must pass for the same proven static), and pacing-gated — not
attacker amplification: no attacker packet reaches the spend without the
application choosing to accept.

The conclusion, stated as capability rather than direction: an attacker who
cannot forge source addresses pays **0 DH in the endpoint core**, and **1 DH
(probe) or 2 DH (probe + authenticate) per packet against any application
that implements a policy** — an application that never probes can never
accept anyone and is not a useful baseline — plus one parked slot per
distinct source (§4's caps). Entering the hint set requires the source
`(IP, port)` of any established connection **or any address we have
dialled**, and the entropy of that requirement is stated honestly: for a
bootstrap or rendezvous peer dialled on a well-known port it is ≈ 0 bits.
Hint-set membership is also *obtainable*, not only guessable: any
key-holding peer can roam its own connection to an arbitrary address by
spoofing the source of an authenticated Data packet (black-holing itself in
the process), planting that address in the hint set. What membership buys is
exactly WireGuard's baseline — 1 DH per mac1-valid packet — which WireGuard
itself only mitigates with cookies/mac2, deferred for slither too (§11).

**No amplification.** The accept path replies 107 B (msg2) to a 196 B
stimulus, and no path in this draft emits bytes in response to an
unauthenticated packet where v1 would not: no cookie replies, no error
packets, `0x04` still never emitted, and every staged rejection is local and
silent.

## 6. The sans-io core *(DRAFT 2026/08/13 — new)*

### Two cores, one poll contract **[MAINTAINER]**

The protocol logic lives in two pure state machines, `core::Endpoint` and
`core::Connection` — v1's proven handshake/session split, and WireGuard's
device/peer split. Each follows the str0m contract: **every mutating call is
followed by draining `poll_output()` until the terminal `Timeout` variant**,
which doubles as the next-deadline announcement — the drain-complete sentinel
and the deadline query are the same thing, so a driver cannot forget to
drain. One deliberate deviation from the quinn precedent: the
connection→endpoint event channel is **folded into `ConnOutput` as a
`ToEndpoint` variant** rather than exposed as a separate poll — slither's
`poll_output` is driver-facing, not application-facing (the application only
ever touches the shell objects of §3–§4), and the driver dispatches every
variant anyway. One drain loop, no second queue to forget.

### Responsibility split

The **endpoint core** owns everything handshake- and demux-shaped: classify
and the oversize drop, mac1, the stage-0 queue and the staged-accept verbs,
the hint map and the internal continuation (§5), **all initiator pendings**
(initial connect *and* rekey: msg1 building, fresh-initiation retransmits
with jitter, the 90 s give-up), index minting and both index tables, the
timestamp state and guard, and the root RNG. The **connection core** owns
exactly v1's `Session` + `Recovery`: seal/open, the replay window and the
`shed_mask` (§8), roaming, the keepalive/liveness timers, the payload age
gates (rekey trigger and backstop), and the whole Leg 2 frame layer.

This split resolves two v1 entanglements structurally: msg1 construction
needs the endpoint-global timestamp monotonicity and the root RNG, so
pendings belong beside them; and **queued sends before establishment stop
being a special mechanism** — a `core::Connection` exists from `connect()`
in the connecting state, and early `send()`s land in the ordinary Leg 2 send
queue (§9), with nothing to flush on completion (v1's `pending.queued`
side-channel dies).

A rekey round-trip: the connection core hits the age gate on a payload seal
→ emits `ToEndpoint::NeedsRekey` → the endpoint starts a pending (ignoring
the request if one is already in flight) → completion feeds
`Install { initial: false }` back → the connection swaps (silent, per-epoch
reset, re-queue). Give-up on a rekey pending is silent; on an initial
connect it emits the shell-only `HandshakeFailed` (below).

### The concrete surfaces

The types below are **normative in shape**; an implementation may rename
fields. `Transmit` is `{ to: SocketAddr, data: Vec<u8> }` (no ECN or GSO at
slither's scale). `core::Endpoint` is **generic over `I: Identity`**
(mirroring v1's `Actor<I, W>` monomorphise-then-erase pattern) — the generic
must reach the core's type, not just the constructor, because the mid-state
map is `HashMap<IntroId, …>` over `I::Provider`-typed hiss mid-states.

```rust
impl<I: Identity> core::Endpoint<I> {
    fn new(now: Instant, config: Config, identity: I, rng_seed: [u8; 32]) -> Self;

    fn connect(&mut self, now: Instant, remote: SocketAddr, remote_static: PublicKey)
        -> Result<(ConnectionId, core::Connection), ConnectError>;   // builds msg1, arms retransmit

    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]) -> Disposition;
    fn handle_timeout(&mut self, now: Instant);                       // idempotent
    fn handle_connection_event(&mut self, id: ConnectionId, ev: ToEndpoint);

    fn poll_output(&mut self) -> EndpointOutput;                      // drain to Timeout

    // staged accept verbs — the shell's Intro/Claimed/Proven handles call these
    fn read_identity(&mut self, intro: IntroId) -> Result<PublicKey, IntroError>;   // 1 DH (0 on a carried entry, §5); Err(Internal) = consumed (§5.4)
    fn authenticate(&mut self, intro: IntroId) -> Result<(PublicKey, Timestamp), AuthError>; // +1 DH; guard admits here
    fn accept(&mut self, now: Instant, intro: IntroId)
        -> Result<(ConnectionId, core::Connection), AcceptError>;     // +2 DH (re-home: +4, §4); msg2 for the freshest parked initiation; returns an ESTABLISHED core
    fn reject(&mut self, intro: IntroId);                             // silent; frees the slot at any stage
}

enum Disposition { ForConnection(ConnectionId), Done }
// ForConnection: the shell feeds the same datagram to that connection core.
// Done: consumed internally (parked, demoted, internal continuation, or dropped).

enum EndpointOutput {
    Transmit(Transmit),                       // msg1/msg2, retransmits, internal-continuation msg2
    IntroReady(IntroId, SocketAddr),          // stage-0 arrival for the accept queue
    ToConnection(ConnectionId, Install),      // the only endpoint→connection event
    HandshakeFailed(ConnectionId, ConnectError), // shell-only: resolves Connecting (below)
    Timeout(Option<Instant>),                 // terminal: drained + next endpoint deadline
}

struct Install { session: EstablishedSession, initial: bool }  // initial=false ⇒ silent swap
```

```rust
impl core::Connection {
    fn handle_datagram(&mut self, now: Instant, src: SocketAddr, datagram: &[u8]);
    fn handle_timeout(&mut self, now: Instant);                       // idempotent
    fn handle_endpoint_event(&mut self, now: Instant, ev: Install);   // Install only — never HandshakeFailed

    fn send(&mut self, now: Instant, msg: &[u8]) -> Result<(), SendError>;            // reliable (tracked DATA)
    fn send_unreliable(&mut self, now: Instant, msg: &[u8]) -> Result<(), SendError>; // untracked DATA (§9)
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
    AddressMoved { to: SocketAddr },  // keeps the hint map + rekey targeting fresh (§5)
    Retired { our_index: u32 },       // teardown: endpoint drops the index route (MUST, below)
}
```

**`accept()` returns a fully established connection — never followed by an
`Install`.** The session is baked into the `core::Connection` at
construction (all 4 DHs paid, msg2 queued for transmit), and
**`poll_output()` never emits `Install` for a `ConnectionId` surfaced via
`accept()`**. `Install` events target only:

- a `connect()`-created (initiator) connection awaiting completion —
  `Install { initial: true }`, exactly once, resolving its `Connecting`
  (whether the session came from msg2 completion or from a lost tie-break's
  responder continuation, §5);
- an established connection being rekeyed — `Install { initial: false }`,
  the silent swap.

An implementation that emitted a symmetry `Install` after `accept()` would
double-install (re-arming timers and double-firing the exactly-once
resolution); one that waited for an `Install` that never comes would hold a
connection that never establishes. Both are excluded by this rule.

**`HandshakeFailed` never reaches `core::Connection`.** It is an
endpoint-level output the shell consumes: the shell resolves the
`Connecting` future with `Err(ConnectError::TimedOut)` and then drops the
never-established pending connection handle. `handle_endpoint_event` is
accordingly narrowed to carry only `Install { session, initial }` — there is
no terminal state a never-established core could meaningfully enter, and no
`ConnectionLost` variant fits a connection that never existed.

**`Timeout(None)`** = drained, and **no deadline is armed**;
`Timeout(Some(d))` = drained, next deadline `d`. The lateness bound `L`
(below) applies to `Some(d)`. The semantics are identical for both cores.

**Output ordering within one drain preserves generation order**: a transmit
and the event it caused come out in that order. This is normative — tests
and logs depend on it.

**`Retired` is a MUST.** Every terminal `ConnOutput` — a
`Closed(ConnectionLost)` event, or the result of `close()` — is followed,
**within the same drain**, by `ToEndpoint::Retired`, and the shell delivers
it to `handle_connection_event` **before** releasing the connection's
shell-side bookkeeping. `Retired` frees the index route and demotes the
connection's timestamp-guard entry from pinned to orphan (§7); releasing the
bookkeeping first would leak both for the life of the endpoint. (The
distinct all-handles-dropped case — driver stops, no teardown at all — is
exempt, §3.)

**The no-blocking invariant.** Every driver→handle delivery is one of: a
bounded channel with an explicit **non-blocking shed policy** (the receive
buffer, §8), or a oneshot reply that cannot block the driver (the staged
verbs, `Connecting`). The accessors — `remote_static()`,
`remote_address()`, `session_id()`, `is_established()` — are **synchronous
reads of a shared cell the driver updates**, never driver round-trips: an
accessor cannot block on the command channel, and the driver never performs
a blocking send toward a handle. This is the architectural rule that makes
the shell deadlock-free by construction, not merely deadlock-free on the
paths analysed here. `session_id()` is the Noise handshake-derived session
identifier the golden vectors pin.

### Time and timers

- **`now: Instant` flows as an explicit argument** on every mutating call;
  the cores never call `Instant::now()` or a wall clock. The initiation
  timestamp is the one wall-clock read, obtained via a clock service
  injected in the endpoint core's config (the v1 `next_timestamp` seam).
  `poll_output` takes no `now`; deadlines are computed from state.
- **Named timers, single min-deadline.** `core::Connection` keeps a fixed
  timer table — `Keepalive`, `PersistentKeepalive`, `Liveness`, `Loss`,
  `Pto` — and exposes `Timeout(min)` as its one deadline (quinn's
  `TimerTable` pattern). `REKEY_AGE` and `REJECT_AGE` are **not timers**:
  they remain payload-path consults (the 2026/07/17 amendment stands). The
  endpoint core's deadline is the min over its pendings'
  retransmit/give-up deadlines and the parked intros' expiry.
- **`handle_timeout` is idempotent**: each due timer is stopped before its
  logic runs, so spurious or repeated calls no-op. This excludes the
  stuck-deadline busy-loop by construction. For `Loss`/`Pto` the idempotency
  additionally rests on the synchronous-sealing rule below.
- **Equal-deadline priorities** (v1's implemented ordering, restated as
  protocol): give-up beats a same-instant retransmit; per connection, **loss
  detection beats PTO and exactly one of the two fires per evaluation**
  (RFC 9002 §6.2); **teardown collection precedes keepalive evaluation — a
  session already collected for teardown owes no keepalive**. (The first
  round of this draft stated the inverse; v1's implemented order —
  `endpoint.rs:904–925`, teardown checked first, keepalive skipped for a
  dead session — is the one restated here, and the overlap is reachable at
  the documented defaults: a session owing a 25 s persistent keepalive has
  already passed the 15 s liveness bound.)

**The lateness bound.**

| Parameter | Value |
|---|---|
| `L` (shell lateness bound) | **250 ms** |

> Every armed deadline `D` fires no earlier than `D` and no later than
> `D + L`. `L` is a **conformance parameter of the shell, not of the
> protocol**: the core exposes exact deadlines
> (`Timeout(Option<Instant>)`, the `Some` arm), and a shell may batch or
> tick provided it honours `L`.

This block-quote is the contract's single normative home; §9 applies it to
the v1 text it replaces by reference.

### RNG

The endpoint core owns one `ChaCha20Rng` seeded from a constructor
`[u8; 32]` (config-supplied for tests, OS entropy otherwise). Every index,
jitter, and — via the forced increment — timestamp draw comes from it. At
connection creation the endpoint draws a 32-byte **sub-seed** and hands it to
the connection core; phase 1 connection cores make no draws, but the sub-seed
is drawn *anyway*, so that adding connection-side randomness later cannot
perturb the endpoint's draw order. One root seed reproduces the whole
system. (This changes v1's single-stream draw *ordering*; no golden test
depends on it, and the timing tests depend only on the jitter distribution.)

### Plan-then-commit sealing

Packetisation is **plan, seal, then commit**: the connection core builds a
packet plan, seals it, and **only on seal success** commits the recovery
transition (dequeue, mark transmitted, clear the pending ACK,
`on_packet_sent`, arm timers). On seal failure the recovery state is
untouched by construction — a seal error can never strand frames outside
both the send queue and the loss tracker. A seal failure is also never
silent: it is only reachable via nonce exhaustion (the reserved `2⁶⁴ − 1`,
v1 §6), and it moves the connection to
`Closed(ConnectionLost::NonceExhausted)` rather than stalling. Seal returns
`(counter, bytes)` — the v1 recover-the-counter-by-reparse artefact dies
(Appendix A).

**Sealing is synchronous.** Plan-seal-commit — `on_packet_sent` included —
executes **within the mutating call that triggers it** (`handle_timeout`,
`handle_datagram`, `send`), never lazily inside `poll_output()`:
`poll_output` only drains already-generated output. This is what makes
`handle_timeout`'s idempotency real for `Loss`/`Pto`: a repeated call before
any drain observes the PTO deadline already advanced by the committed probe,
so `on_pto` cannot double-fire and corrupt the RFC 9002 backoff schedule.

## 7. Endpoint-global state *(DRAFT 2026/08/13 — new)*

Four pieces of state are endpoint-core-global; none may be pushed into a
connection.

1. **The timestamp guard** — per-remote-static greatest timestamp (v1 §5
   Responder 5). Per-static scope is confirmed by all three WireGuard
   implementations; per-connection scope would let a peer replay a msg1 into
   a second attempt. Admission (check **and** record) happens at
   `authenticate()` for the staged path (a re-homed `accept()` admits its
   fresh initiation's timestamp the same way, §4) and inside the internal
   continuation for replacements — all post-`ss`, so only key-holders write
   entries, and in the continuation the record is made **only on full
   admission** (§5).
2. **`last_init_timestamp`** — the endpoint-global outbound monotonic
   forcing (v1 §5). It must survive across connection generations to the
   same peer (close and reconnect must still emit strictly greater), so
   endpoint scope is the correct superset.
3. **The index demux** — `index → connection` and `pending-index →
   connection`. **Index minting (session and pending alike) draws a random
   nonzero `u32` and re-draws while the value is present in *either*
   table.** The nonzero rule and the uniqueness rule are both protocol; the
   randomness source is the injected root RNG (§6). (Required across both
   tables because a pending's msg1 index graduates into the session index on
   completion; this closes v1's unchecked-insert route stealing.) The
   corollary is stated rather than implied: **a datagram that routes by
   index but fails to open touches neither liveness, nor roaming, nor the
   replay window** — §8's decrypt-first ordering guarantees it, and it
   matters because a freed-then-re-drawn index makes stale traffic land on
   the wrong connection routinely; such traffic is inert.
4. **The static → connection map** (§3) plus the derived **hint map**
   (connection → current endpoint address), maintained by the connections'
   `AddressMoved` events.

### Bounding the timestamp guard

Under application-driven accept the guard map is no longer bounded by an
allow-list. The rule:

- An entry is **pinned** — never evicted — while a live `Connection`, an
  in-flight outbound pending, or a staged mid-state exists for that static.
  The pin *creates* the entry for a `connect()`-dialled static or an
  established connection (caller-chosen or possession-proven names — this is
  also where the §5 pacing counter lives, so the counter exists for every
  continuation trigger). For a staged mid-state — whose static is merely
  *claimed* until `authenticate()` — the pin never creates an entry: it is a
  **bounded exception** to §4's nothing-durable-keyed-on-a-claim rule,
  flipping a bit on an entry a key-holder already wrote, reverting on drop,
  bounded by the intro-queue cap.
- All other entries (orphans: dead connections) live in a **bounded LRU
  with timer aging** (below):

| Constant | Value | Notes |
|---|---|---|
| `TS_GUARD_ORPHAN_CAP` | **1024** orphan entries | configurable; ~45 B each, ~50 KB at the default cap; default pending the phase-1 flood validation (draft-note, removable at ratification) |

**Honesty clause — the eviction consequence [MAINTAINER].** Evicting an
orphan re-admits a replay of that static's last initiation. The replayed msg1
is genuine, so it authenticates, and can surface as a fresh `Intro` — or,
if accepted, produce a half-open session whose msg2 answers nobody, reaped by
liveness in 15 s. This is exactly the exposure WireGuard accepts on responder
restart ("an initial packet from earlier can be replayed, but it could not
possibly disrupt any ongoing secure sessions") — and pinning guarantees the
eviction can never touch an *established* connection's replacement
protection. The clause is completed rather than left incidental: **eviction
is attacker-triggerable on demand** — orphan entries require a key, but the
attacker supplies the keys (self-generated statics are free), so ~1024
authenticate-then-drop chains, at ~2048 DH of **our** cost, would flush the
orphan tier; **LRU order is adversarially optimal**, evicting the
longest-idle *legitimate* peers first — precisely those whose old
initiations an observer is most likely to hold; and the observable
consequence, one step further than "a fresh `Intro`": an accepted replay
surfaces as a **spurious `Connection`/`Intro` attributed to a real peer at
an attacker-chosen address** — it dies at 15 s liveness and leaks no key
material, but any application side-effect keyed on "peer is online" fires on
a forgery.

**Mitigations ruled in [MAINTAINER].** Two changes keep the orphan tier from
being attacker-steerable: (i) **a static that is authenticated and then
rejected without ever being accepted writes no orphan** — its guard record
is dropped with the chain (an entry that pre-existed the chain reverts to
its prior state), so the authenticate-then-drop flood cannot mint orphans at
all; the cost, stated honestly, is that a replay of such a never-accepted
initiation can be re-authenticated later — it surfaces only as a fresh
`Intro` requiring another application decision, the same exposure as an
eviction, and pinning still protects every established connection. (ii)
**Orphans age out on an `INTRO_TTL`-scale timer** as well as the LRU cap, so
attacker volume does not translate into eviction of durable entries.
**LRU "use" is admission only**: an entry's recency is refreshed by a
successful post-`ss` record — never by a failed guard *check* — keeping the
write path key-holder-only; replayed (rejected) msg1s cannot keep an orphan
warm.

Phase 1 keeps the guard as one map with the pinned/orphan distinction. Its
natural future home, if per-peer state grows, is a per-known-static record
tier in the WireGuard shape (`wg_peer`); the pacing counters of §5 already
live in the same map's values.

### Endpoint state ceilings

The composite bound, stated in one place so an application can size its
accept policy:

| State | Ceiling | Worst case |
|---|---|---|
| Stage-0 entries + consumed chains | one budget of `INTRO_QUEUE_CAP` slots (§4) | ~220 B raw bytes each, ~225 KB |
| Staged mid-states (consumed chains + carried pre-read entries) | ≤ `INTRO_QUEUE_CAP` | ~0.5–1 KB live key material each, ≈ 1 MB — and each holds the endpoint's static provider: **for a hardware/enclave static this is up to 1024 concurrent provider handles**, an operationally scarce resource the TTL bounds in time |
| Timestamp-guard map | `TS_GUARD_ORPHAN_CAP` orphans + pinned entries (one per mid-state ≤ the queue cap, one per connection) | ~45 B each |
| Established connections | **application-governed — unbounded by the protocol** | per-connection receive buffer `RECV_BUFFER` × `MAX_MESSAGE` ≈ 297 KB (§8) — the dominant memory term |

## 8. Session behaviour amendments *(DRAFT 2026/08/13 — amends v1 §6)*

### The swap cuts the old session instantly **[MAINTAINER]**

v1's implemented behaviour is ratified for v2 and now stated: the moment a
replacement session installs (rekey completion or responder-side
replacement), **the old session's index and keys are dropped; in-flight
packets sealed under the old session die**. This knowingly diverges from
WireGuard's previous-keypair retention. Leg 2 makes the cut safe:
undelivered messages re-queue onto the fresh session (v1 §9.5), the seq
space and receiver dedup survive the swap, so **nothing is lost or
duplicated once the replacement completes on both sides**; a lost msg2
leaves the connection one-way-dark until the next initiation (~5 s), against
the 15 s liveness budget. The only unrecoverable casualties are keepalives,
which are expendable. A grace window would buy latency at the price of two
live receive sessions, ACK-source ambiguity (the old counter space owes no
ACK after the per-epoch reset — grace-received DATA could be delivered but
never acknowledged, forcing its retransmission anyway), and roaming/liveness
attribution questions. Revisit with streams if the retransmit cost shows up
in practice (§11).

### Epoch death is subsumed by liveness

`MAX_EPOCH_JUMP = 2` (v1 §6, hiss-fixed) makes a peer more than two epochs
(131 072 counters) ahead permanently unopenable — the receive side refuses
without deriving keys. **No dedicated detection or recovery path exists or
may be added.** A legitimate peer can only get there across ≥ 65 536 messages
of silence, which `DEAD_TIMEOUT` (15 s) excludes by orders of magnitude. If
the condition somehow arises, no inbound packet opens, `last_recv` stops
advancing, and the liveness timer tears the session down — the correct
outcome via the existing mechanism. **The epoch ratchet's far-future refusal
is subsumed by liveness; implementations must not chase epochs.** At the
hiss surface the refusal is a generic decryption failure — **verified
against hiss 0.3.1's `src/noise/datagram.rs`**: the far-future-epoch refusal
returns `HandshakeError::DecryptionFailed`, the same variant as any failed
open — so it is genuinely indistinguishable from any other failed open, and
no dedicated trace for it is promised or possible without a hiss
error-granularity change. (Round-2 research item 4: resolved.)

### The replay window stays at 128

`REPLAY_WINDOW = 128` is kept for 0.2.0. The wire coupling forces it:
`MAX_ACK_RANGES = 63` is *derived* from the 128-bit window (the worst-case
alternating pattern), a maximal ACK is 268 B, and a v1 peer **rejects an ACK
with more than 63 ranges as a protocol violation** (v1 §9.2/§9.6, pinned by
`over_cap_ack_is_rejected`). Widening to boringtun's 1024 would make the
worst-case ACK 511 ranges ≈ 2 KB — unrepresentable in one frame under
`MAX_PLAINTEXT` and fatal to any 0.1.0 peer — so widening is a wire
revision, excluded by the frozen-bytes invariant. The references sit far
wider (kernel 8128 usable bits, boringtun 1024), and the divergence has a
real cost, stated plainly: **reordering beyond 128 packets drops the
stragglers at the window, which Leg 2 converts into spurious
retransmissions — not loss**. Revisit in the streams/CC round, where a new
ACK format can ride the reserved frame space (§11).

### Roaming observability

v1 §6's roaming rule stands, amended only as stated here and in the
backpressure clause below: **handshake packets never roam a live session** —
only an authenticated, fresh, **window-marked** Data packet moves the
endpoint (an accepted initiation *anchors* a new or replacement session at
its msg1 source, which is not roaming). The `EndpointMoved` event dies with
the event stream: observability is the `remote_address()` accessor plus the
**`slither::roam`** trace target (§10); at the core level the connection
emits `ToEndpoint::AddressMoved` so the endpoint's hint map and rekey
targeting stay fresh (§5).

### Per-connection persistent keepalive

`set_persistent_keepalive(Option<Duration>)` is **per-connection** (default
off), superseding v1's endpoint-wide `Config` knob. This entails a stated
reclassification of a ratified table entry: `PERSISTENT_KEEPALIVE = 25 s` is
**reclassified from a frozen constant to the recommended default**, forced
by the interval becoming caller-chosen `Option<Duration>`; the change is
wire-invisible. All other timer **values** in the v1 §6 table are unchanged;
their evaluation contract is §6's lateness bound.

### Receive backpressure

| Constant | Value | Notes |
|---|---|---|
| `RECV_BUFFER` | **256** messages per connection | configurable; default pending the phase-1 `acks_keep_flow_over_a_lossy_wire` throughput validation (draft-note, removable at ratification) |

The unbounded v1 event stream is replaced by a bounded per-connection
receive buffer — and the v1 replay invariant is preserved under it: **the
replay window is marked on every authenticated, fresh packet, shed or
delivered**, and **liveness and roaming are driven only by a packet that is
both authenticated and window-marked**. No replayed packet ever moves the
endpoint or refreshes liveness, backpressure or not — v1 §6's ratified rule,
carried without exception. (An earlier draft left shed packets unmarked,
which let a passive observer replay a recorded shed packet from a spoofed
source and roam the endpoint; marking every fresh packet closes that hole
*and* means replay detection is never suspended under sustained
backpressure.)

The inbound sequence: decrypt → replay **check**; already-seen ⇒ drop
without delivery (v1 §6) → fresh ⇒ replay **mark**, then liveness update and
roaming → **room for every DATA frame aboard the packet?** deliver and
schedule the ACK : set the packet's counter bit in `shed_mask` and deliver
nothing. **"Room" is whole-packet**: if the buffer cannot hold all the
packet's DATA frames, the whole packet is shed — marked in the window, never
partially delivered, never acknowledged. Partial delivery would acknowledge
DATA the application never received: exactly the permanent-loss bug the rule
exists to exclude.

**`shed_mask`** is a second 128-bit mask alongside the replay window —
16 bytes per connection, a pure-core change — indexed identically to the
window bitmap and sliding with it. A shed counter is marked in the window
(replay protection holds) and its `shed_mask` bit records that its frames
were never delivered. **ACK-range construction operates on the effective
acknowledged set — the window bitmap AND NOT `shed_mask`** (the §2
carve-out on v1 §9.2's ACK-from-the-window derivation). Every ACK field
derives from that masked set: **`largest` is the greatest
marked-and-not-shed counter** — not the greatest window counter, which may
itself be shed — and `first_range` and every subsequent range are computed
over the masked bitmap; `AckFrame::from_window` (or its equivalent) takes
the masked snapshot, never the raw window. A shed counter therefore never
appears as the ACK `largest` nor inside any range — acknowledging it would
clear DATA the application never received at the sender: permanent loss.
Left un-ACKed, the peer's loss detection retransmits the frames on a
**fresh** counter (a new window bit), delivered when the buffer drains;
exactly-once holds via seq dedup. A counter that slides off the window
slides off the mask with it — by then its retransmission has long since
happened or the connection is dead.

This requires splitting the window's admit into *check* and *mark* — a
pure-core change (`admit` survives as a thin wrapper, Appendix B). The
accept queue's bound is §4; the handle→driver command channels are bounded,
small, and exert natural `await` backpressure; the delivery path itself is
the non-blocking shed policy §6's no-blocking invariant names.

## 9. Leg 2 amendments *(DRAFT 2026/08/13 — amends v1 §9.4, §9.5, §9.7)*

### The tick sentence is replaced

v1 §9.4's closing sentence — "Both timers are evaluated on the actor's
250 ms TICK" — is **deleted**. In its place, the §6 lateness-bound contract
applies **by reference** (§6 is its single normative home; this section
deliberately does not restate it — the banner's own rule is that a
restatement can drift). `K_GRANULARITY` (1 ms) is unchanged; v1 §9.6's
parenthetical "(the 250 ms TICK bounds it in practice)" is rewritten to cite
`L`.

### `send`, `send_unreliable`, and the `Reliable` boundary

The frame layer (Leg 2) **always runs in the connection core** — it must:
ACK ranges come from the Leg 1 replay window, control seals are
liveness-neutral, and PTO/loss need counters; none of that is buildable
outside the core on the frozen wire. `Reliable<Connection>` is therefore an
**API selector, not a protocol layer**:

- `Connection::send_unreliable(&[u8])` — a DATA frame with a fresh seq that
  is **never tracked for loss**: fire-and-forget, at-most-once. (The frozen
  wire has no other non-frame plaintext — a raw unframed payload would be a
  protocol violation at the peer.) The frame's `seq` is drawn from **the
  same monotonic sequence space as `Reliable::send`'s**, skipping only the
  retransmission-queue insert: the wire's `DataFrameHeader` carries no
  reliable/unreliable flag and the receiver's dedup is keyed purely on
  `seq`, so a private counter would collide with the tracked path and
  silently swallow one message as a duplicate of the other. `Recovery` grows
  the matching **allocate-seq-without-tracking** primitive (the tracked
  path's queueing allocates and tracks in one step); the
  reliable/unreliable interleaving test is an Appendix B obligation. The
  receiver cannot and need not distinguish; dedup and exactly-once surfacing
  apply as normal.
- `Reliable::send(&[u8])` — the tracked path: queued, retransmitted, ACKed,
  exactly-once within the connection's life (v1 §9.3 unchanged).
  `Reliable::new(conn)` / `into_inner()`.
- `Connection::recv() -> Result<Vec<u8>, ConnectionLost>` — messages surface
  on the base connection regardless of which path the peer used. Receive
  semantics are identical by construction, so **`Reliable` adds nothing on
  receive**; its `recv` forwards to the base.
- **`MAX_MESSAGE` (1159) is checked at the handle, before any queue** —
  `SendError::PayloadTooLarge`, never enqueued.

**Sends before establishment** are ordinary sends: the connection core
exists from `connect()` (§6), and `send()` queues into the Leg 2 send queue
(`to_send`), pumping when a session installs — delivered exactly once after
establishment, lost if the connect fails (with the failure surfaced through
`Connecting`, §5).

### Restated interplay rules (v1 §9.5, §9.7 — values unchanged)

The event-stream language of v1 dies (§10); the behaviour it described does
not. Restated in v2 terms:

- **The liveness-neutral seal set** (`seal_quiet`): pure ACKs, PTO probes,
  and retransmissions. **Only fresh application sends and the keepalive mark
  `last_send`.** Inbound opening is age-exempt; quiet control is age-exempt
  (the 2026/07/17 amendment stands).
- **Keepalives are admitted to the replay window but bypass the frame layer
  entirely**: they appear in later ACK ranges with `ack_delay = 0` and never
  reach recovery.
- **Every ACK derives from the effective acknowledged set** — the window
  bitmap AND NOT `shed_mask` (§8): `largest` is the greatest
  marked-and-not-shed counter, and every range is computed over the masked
  bitmap, so a shed counter never appears in an emitted ACK.
- **An empty `send` is a real (empty) message; an empty *plaintext* is the
  keepalive.** The two must not re-merge (v1 §9.7).
- **Undelivered messages die with the connection** (`ConnectionLost`,
  `close()`, dropping the handles) **but survive a rekey** — the
  undelivered set re-queues onto the fresh session (v1 §9.3/§9.5).

## 10. Error taxonomy *(DRAFT 2026/08/13 — replaces v1 §7's `Failed` bullet and every event mention)*

The global `Event` stream, `ConnId`, `SessionHandle`, `allow`/`revoke`, and
all five v1 events dissolve:

| v1 | v2 |
|---|---|
| `Established` | `Connecting` resolving / `Proven::accept()` returning — exactly once (§5) |
| `Failed { TimedOut }` | `Connecting` resolves `Err(ConnectError::TimedOut)` |
| `Incoming { payload }` | `Connection::recv().await` |
| `Dead` | `recv()`/`send()` return `Err(ConnectionLost::…)` |
| `EndpointMoved` | `remote_address()` accessor + `slither::roam` trace |
| `allow`/`revoke` + allow-list | dissolved: policy = the application's staged decision (§4); "revoke" = drop the `Connection` |

The staged errors are §4's **three** types — `IntroError`, `AuthError`, and
`AcceptError`. The normative taxonomy, closed for phase 1 — every variant
this specification names appears here, exactly once each, and **`Superseded`
appears nowhere** (§4 [MAINTAINER], own-bytes-on-consume made it
unreachable):

- `IntroError::{Expired, Internal, Malformed, EndpointDropped}` — `Expired`:
  the parked entry outlived `INTRO_TTL`; `Internal`: the §5 interception —
  the initiation belonged to a known static and was consumed by the
  endpoint; discard the handle; `Malformed`: the msg1 read fails
  structurally; `EndpointDropped`: the driver task stopped mid round-trip.
- `AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}` —
  `Replay`: the automatic guard failure; `HandshakeFailed`: the
  tail-tag/crypto failure of a forged claim — **the only variant in the
  staged taxonomy that is a security signal**; the rest are liveness and
  lifecycle; `Expired`: the parked attempt aged out at this stage;
  `EndpointDropped`: as above.
- `AcceptError::{Stale, AlreadyConnected, EndpointDropped}` — `Stale`: no
  initiation for the proven static is currently parked — the peer stopped
  retrying, or every parked one aged out (§4's re-home rule; the
  application SHOULD re-accept when the peer's next initiation surfaces as
  a new `Intro`); `AlreadyConnected`: a session for this static installed
  meanwhile (§3's one-connection-per-static rule); `EndpointDropped`: as
  above. There is no `Expired` at this stage: a chain's age never fails an
  `accept()`.
- `ConnectError::{AlreadyConnected, TimedOut}` — **closed** for phase 1.
  `AlreadyConnected`: §3; `TimedOut`: initial-connect give-up at
  `HANDSHAKE_GIVEUP`.
- `ConnectionLost::{TimedOut, RekeyFailed, NonceExhausted, LocallyClosed,
  EndpointDropped}` — `TimedOut`: liveness (15 s); `RekeyFailed`: the 180 s
  payload backstop; `NonceExhausted`: the §6 seal-failure death;
  `LocallyClosed`: `close()` or the last-handle drop, observed by a
  surviving clone; `EndpointDropped`: the driver stopped.
- `SendError::{PayloadTooLarge, ConnectionLost(ConnectionLost)}`.

**Trace targets are operator-visible contract** and survive the rewrite:
`slither::policy` — which outlives the allow-list it once traced and now
records guard rejections, pacing rejections, and internal-continuation
outcomes (admissions, tag deaths, and tie-break drops) — `slither::replay`,
`slither::frames`, plus the new `slither::roam`.

## 11. Out of scope (phase 1) and known limitations *(DRAFT 2026/08/13)*

Phases 2–4 are unchanged by this draft: suite genericity (`Channel`,
`channel!`, `Identity<C>`, generic mac1); the `Ordered` wrapper, `split()`,
and futures `Stream`/`Sink`; streams + congestion control in the reserved
frame space. Deferred, with pointers:

- **Cookies/mac2** (v1 §4's reserved `0x05`) — the answer to §4's
  queue-occupancy exposure and §5's hint-set spoof, both stated there;
  deferred to the streams/CC round with WireGuard's under-load model as the
  template.
- **Per-peer `ss` precomputation** — WireGuard amortises `ss` to
  peer-configuration time, making a known peer's msg1 cost one live DH.
  hiss has no seam for a precomputed `ss`. Two recorded options: (a) a hiss
  `complete_with_precomputed_ss` variant; (b) a memoising `DhProvider`
  wrapper keyed on the peer public — possible today at the provider seam,
  but its cache must be populated only for proven peers and bounded, or it
  becomes attacker-controlled allocation. The §4 DH-cost table would need
  re-ratification either way.
- **Replay-window widening** — excluded by the §8 derivation chain; needs a
  new ACK format in the reserved frame space.
- **A swap grace window** — declined in §8; revisit with streams.

**Known behaviour — `accept()` re-homes; the accept-commitment model is
deferred [MAINTAINER].** `INTRO_TTL` was never the real bound on a slow
accept decision: fresh-ephemeral retransmit re-mints the initiator's index
every ~5 s (`endpoint.rs:993–1023`), and a msg2 answering a superseded
initiation is ignored, so a `Proven` chain's own initiation goes stale in
about one retransmit interval. §4's re-home rule covers every decision made
while the peer is still retrying — its full 90 s give-up window; a decision
slower than that returns `AcceptError::Stale` and waits for the peer's next
initiation to surface as a fresh `Intro`. The cost is §5's re-home note
(the inspected chain's `es` + `ss` plus the fresh initiation's
`es` + `ss` + `ee` + `se`). The richer model — an **accept commitment**
that auto-completes the *next* arriving initiation from the proven static,
fully decoupling arbitrarily slow decisions from the peer's retry
schedule — is named and deferred: it needs a committed-static table with
its own bounds and eviction story. `INTRO_TTL = 15 s` stays on the
unconsumed tier as the DoS defense it is.

**Known limitation — the peer-restart seq collision [MAINTAINER].**
Replace-in-place keeps our receiver dedup floor across the swap (v1 §9.5).
A peer that *restarts* (rather than rekeys) re-sends seqs from 0, and we
silently swallow them as duplicates — a v1 behaviour v2 preserves, because
rekey and restart are cryptographically indistinguishable at msg1 time. And
because ACK ranges are built from the replay window (v1 §9.2), the swallowed
messages are **acknowledged**: the restarted peer's recovery layer clears
them as delivered. **The loss is silent, confirmed, and undetectable at both
ends.** A timestamp-gap heuristic is unsound; the sound fix is an explicit
epoch in a future frame, which is wire work slated for the streams round.
Phase 1 documents the limitation and changes nothing.

## 12. Appendix A — hiss dependencies *(DRAFT 2026/08/13; non-normative)*

Phase 1 depends on one hiss 0.3.x minor carrying two additions. The split
read is zero-runtime-change **`hiss-macros` codegen** (~300–450 additive
lines) and byte-compatible by construction: it re-uses the identical
`support::*` calls in the identical order, so the transcript, the golden
wire bytes, and the session id are unchanged. `next_counter()` is **not**
codegen: it is a **runtime accessor** exposing an existing counter — trivial
(`CipherState`'s nonce is already crate-visible), but a different kind of
change, and the pair should not be described as jointly codegen-only. With
the split, slither's staged path consumes exactly one provider draw per
accepted handshake (the same as v1's `accept_init`), so the golden vectors
are unmoved by construction.

**1. The split msg1 read.**

```rust
IKResponderMsg1::read_message_1_intro(&msg1) -> Result<(Pk, IKResponderMsg1Mid), _>;
    // e, es, s — 1 DH; the returned Pk is the CLAIMED static
IKResponderMsg1Mid::complete() -> Result<([u8; 12], IKResponderMsg2), _>;
    // ss + tail decrypt — +1 DH; the timestamp only ever emerges here
IKResponderMsg1Mid::claimed_static(&self) -> &Pk;   // named claimed_, never remote_
```

Details fixed for the proposal: the un-read tail carrier is **option (a)**,
an owned `[u8; __MSG1_TAIL_SIZE]` inside the mid state — no lifetime, no
re-supply. The `__MSG1_TAIL_SIZE` const must come from the prefix-tokens
`WireSize` computation under the macro's no-size-arithmetic rule; **if that
const proves fiddly, the fallback is option (c)**, `complete(self, &msg1)`
with the caller re-supplying the same message — a mismatch is caught by the
tail tag (it degrades to a decryption failure), but it is a footgun and the
option-(a) shape is preferred. The mid state is non-`Clone`. A
`hiss::noise::Claimed<K>` newtype is nice-to-have, not required — slither's
own `Claimed` typestate carries the claimed-vs-proven semantics regardless.

Why the fallback (re-reading msg1 with a recording closure, then again with
an accepting one) is dead rather than a bridge: it makes the accepted msg1
read cost three DHs (`es`, `es`, `ss`) against the ratified two, breaking
the DH-cost pins; its throwaway first read costs an extra
`Identity::provider()` draw, moving `GOLDEN_RESP`/`GOLDEN_SID` unless
mitigated by machinery we would then delete; and hardware statics would pay
the state rebuild (a `public_key` call — potentially an enclave round-trip)
twice per accepted handshake.

**2. `DatagramSend::next_counter() -> u64`.** Folded into the same minor.
It deletes slither's mirrored send counter and its `debug_assert`. The
*normative* statement is behavioural only: **the DataHeader carries exactly
the counter the seal used, and the 14 header bytes are the AEAD associated
data** (v1 §3); whether an implementation mirrors the counter or reads the
accessor is not normative. The split read is a codegen-only concern;
`next_counter()` is a runtime accessor — only the former is macro work.

## 13. Appendix B — test migration *(DRAFT 2026/08/13; non-normative)*

**handshake.rs is replaced, and four pins move with it [MAINTAINER].** The
one-shot allow-list-shaped functions — `accept_init`, `build_init`,
`complete_init`, `RespAccept` — are **replaced** by the split-read staged
primitives the endpoint core needs (§4, Appendix A); they do not survive as
a legacy layer (their entire design is the allow-list closure §4 dissolves).
Four tests that call `accept_init` directly are therefore reclassified from
survives-verbatim to **harness-rewrite**:
`golden_wire_is_byte_identical_to_the_pre_migration_driver` (still the
phase's verdict test — its golden hex is untouched),
`msg_sizes_match_the_wire_pins`,
`tampered_payload_fails_then_clean_msg1_succeeds`, and
`responder_dh_cost_is_staged` — same golden bytes and DH assertions, new
call sites via the staged primitives. By contrast, `ReplayWindow::admit`
and `Recovery::next_packet` are **kept as thin wrappers** over the new
check/mark (§8) and plan/commit (§6) split primitives, so the `window_*`
and `recovery::` unit tests need no change.

**Pins that survive verbatim** (byte-for-byte, no harness change):
`rekey_transform_kat`, `protocol_name_is_pinned`,
`timestamp_is_not_on_the_wire`, the handshake round-trip and epoch-boundary
tests, `timestamp_guard_rejects_non_greater`, every `wire::`, `mac::`,
`frame::`, and `session::tests::window_*` pin, the compile-time size
asserts, and all **ten** `recovery::` tests. All are independent of the
actor.

**Flow tests that rewrite harness-only** (behaviour preserved, API driving
rewritten from `Event`/`ConnId` to objects and futures): `happy_path`,
`msg1_lost_twice_retransmits_fresh`, `msg2_lost_reaccepted`,
`handshake_gives_up` (the `Failed` event becomes `Connecting` resolving
`Err`), `mac1_flood_never_reaches_dh`, `initiation_replay`,
`data_through_reorder_and_dup_exactly_once`, `keepalive_after_idle`,
`dead_after_silence`, `rekey_keeps_flow`,
`idle_survives_past_reject_age_then_a_payload_send_backstops`,
`retransmit_into_partition_dies_by_liveness`, `size_caps`,
`unknown_packets_dropped`, `real_udp_loopback`, and the whole
`flow_frames::` set. `roaming_follows_authenticated` survives **as-is** (it
drives `Session` directly).

**Flow tests that rewrite as staged-policy tests**:
`allow_list_rejects_unlisted` becomes "drop the `Intro` ⇒ nothing
transmitted, 0 DH"; `unlisted_initiator_costs_one_dh_and_no_resp` becomes
the §4 cost table driven end-to-end — "`read_identity()` then drop ⇒ exactly
1 DH, no msg2; a subsequent accepted handshake adds exactly
`es + ss + ee + se`".

**Timer windows under the lateness bound `L`.** The existing windows were
derived as `[D, D + tick + jitter-slack]` and remain valid for any
implementation at least as prompt: keep `[10 s, 11.5 s]` (two-loss
establish), `[10, 10.6]` (keepalive), `[15, 15.6]` (liveness), `[90, 90.5]`
(give-up), and `[120, 180)` (rekey trigger) as-is. The one exception:
`asymmetric_loss_retransmit_gate_rekeys_then_backstops`'s 205 s upper bound
encodes the tick-aligned PTO consult schedule; with exact timers the bound
becomes **`180 s + (the PTO interval in force at 180 s)`**, re-derived from
the doubling series at test-writing time — this appendix states the
mechanism, not the number.

**New obligations** (tests this draft's normative clauses demand): the
stage-0 queue (cap, addr-only dedup with replace-with-newest and one
surfacing per source, **evict-oldest overflow**, the **per-source cap**
(counting consumed + unconsumed chains together; `read_identity()`
net-zero for its source), TTL expiry, and **own-bytes-on-consume**: a consumed chain is unsupersedable
and a post-consumption initiation parks as a new entry with a new
`IntroReady`); hint routing (eager continuation, unknown-claim demotion
**with the carried mid-state — `read_identity()` at 0 incremental DH, 1 DH
total for the class**, `read_identity` interception → `Internal`); the
**simultaneous-open tie-break driven from both ends, in both static
orderings, converging on one shared session — and gated post-`ss`: a
forged msg1 claiming the dialled static dies at the tag with the pending
untouched**; the pacing gate (20 ms
spacing, per known static); guard pinning, **no-orphan-on-reject**, orphan
timer aging, and LRU eviction; index re-draw across both tables; the
receive-buffer shed rule (**window-marked + `shed_mask` set, never
delivered, never ACKed, a replayed shed packet rejected at the window,
whole-packet granularity**, retransmitted-and-delivered-once, **and the
masked-ACK derivation: shed the window's greatest counter under sustained
backpressure and assert the emitted ACK's `largest` is the greatest unshed
marked counter, the shed counter absent from every range**);
plan-then-commit on seal failure; **reliable/unreliable seq interleaving**
(no dedup collision); **one completion attempt per retransmit interval**
(a forged msg2 spends the attempt; no immediate re-arm — the next
scheduled retransmit refreshes it); and `AlreadyConnected` (both
`ConnectError` and `AcceptError` forms).

---

## Round-2 changes applied *(non-normative index for the re-review)*

Every ruling of `round2-resolutions.md`, by group, with the section it
landed in:

**Group 1 — receive backpressure.** §8 *Receive backpressure* rewritten:
the window is marked on every authenticated fresh packet, shed or delivered;
liveness/roaming driven only by authenticated **and window-marked** packets;
the 128-bit `shed_mask` (16 B/connection) records shed counters and is
subtracted from the window snapshot at ACK construction; "room" is
whole-packet (every DATA frame aboard, or shed entire); the processing
sequence is stated in the ruled order; the replay-suspended-under-
backpressure concern is resolved (not documented) by the marking rule. §2's
v1 §6 row stops claiming "in full" and names the observability + shed
carve-outs; §2's §9–§9.3 row carries the `shed_mask` ACK carve-out. §6's
responsibility split names the `shed_mask` beside the window. Appendix B's
shed obligations updated to the marked-plus-masked semantics.

**Group 2 — simultaneous open.** §5 *Simultaneous open — the deterministic
tie-break*: lexicographically smaller 33-byte compressed static (unsigned
big-endian octet comparison) is the winning initiator; the winner silently
drops the inbound msg1 pre-`ss` (no guard record); the loser cancels its
pending at the tie-break decision (pre-`ss`) and responds; "resolves to
whichever completes first" deleted (with the mutual-blackout reason stated);
decided at the claimed-static match on both the eager and interception
paths; equal statics unrepresentable (§3 notes `connect()`-to-self is out of
scope); the loser's `Connecting` resolves on the continuation's
`Install { initial: true }`; the continuation's numbered order restates the
cancellation point. Appendix B gains the both-orderings convergence test.

**Group 3 — stage-0 queue DoS [MAINTAINER].** §4: overflow is evict-oldest
(by park time, unconsumed entries only); `INTRO_MAX_PER_SOURCE = 4` per
source IP (per /64 for IPv6), unconsumed entries only; `INTRO_TTL` cut from
90 s to **15 s** (with the initiator-horizon rationale); the honesty clause
rewritten — no spoofing needed (ports suffice), endpoint-wide denial of new
inbound accepts, the ≈ 68 pps sustaining rate, and an explicit
not-WireGuard-equivalent statement; all three queue constants carry the
configurable-default draft-note pending phase-1 flood validation; the ruled
posture is flagged [MAINTAINER].

**Group 4 — own-bytes-on-consume [MAINTAINER].** §4 *Consumption and
supersession*: unconsumed entries are transparently replaced (same
`IntroId`, newest bytes, accessors read newest at call time, no second
`IntroReady`); `read_identity()` transfers ownership of bytes and `IntroId`
and frees the source slot; consumed chains can never be superseded;
**`Superseded` removed from every error enum** (flagged [MAINTAINER], with
the divergence from both reviewers' literal fix noted); the single-budget
state accounting stated (unconsumed + consumed within `INTRO_QUEUE_CAP`;
per-source cap and evict-oldest apply to unconsumed only); the
`Claimed`-plus-fresh-`Intro` edge noted with its
`AcceptError::AlreadyConnected` resolution. §10 confirms no `Superseded`
anywhere.

**Group 5 — eager-demote carries the paid mid-state.** §5 step 3: an
unknown-claim demotion parks the raw packet **with** its mid-state, tagged
identity-already-read; `read_identity()` on it costs 0 incremental DH;
the class costs 1 DH per attacker packet (DoS table row fixed); carried
entries bounded by hint-set sources and the queue cap; §4's typestate table
gains the pre-read footnote (†) and its stage-0-only parking clause the
bounded exception.

**Group 6 — accept()/Install/HandshakeFailed.** §6: `accept()` returns a
fully established `core::Connection`, never followed by an `Install`;
`Install` targets only connect-created completions (`initial: true`, exactly
once) and rekeys (`initial: false`); both wrong-guess failure modes named
and excluded; `HandshakeFailed` is a shell-only `EndpointOutput` variant
(never routed to a core) that resolves `Connecting` with
`Err(ConnectError::TimedOut)`; `handle_endpoint_event` narrowed to
`Install { session, initial }`.

**Group 7 — error taxonomy.** §10 rebuilt as the closed normative taxonomy:
`IntroError::{Expired, Internal, Malformed, EndpointDropped}`,
`AuthError::{Replay, HandshakeFailed, Expired, EndpointDropped}` (with
`HandshakeFailed` marked the only security-signal variant),
`AcceptError::{Expired, AlreadyConnected, EndpointDropped}` (the `Expired`
variant later replaced by `Stale` — see the Round-3 index, F5),
`ConnectError::{AlreadyConnected, TimedOut}` (closed),
`ConnectionLost::{TimedOut, RekeyFailed, NonceExhausted, LocallyClosed,
EndpointDropped}` (all five glossed),
`SendError::{PayloadTooLarge, ConnectionLost(ConnectionLost)}`; §10 names
all three staged error types; §4's error paragraph matches and defers to
§10 as the normative home.

**Group 8 — security honesty + DoS repairs.** §5 DoS table rebuilt
(demote row → 1 DH; new replayed-genuine-msg1 2-DH row, not pacing-capped;
the genuine-replacement row split into uncapped 2 DH + accepted-only
2 DH; the explicit 2-DH single-packet ceiling). §5's conclusion restated as
capability (0 DH core / 1–2 DH against a policy-implementing application),
with the `(IP, port)`-or-dialled-address correction, the ≈ 0-bit entropy
note, and the roam-obtainable hint-set note. §7's guard-eviction clause
completed (attacker-triggerable on demand, adversarially optimal LRU,
spurious-`Connection` observable) with the ruled mitigations —
no-orphan-on-reject and timer aging — flagged [MAINTAINER], and LRU "use"
defined as admission-only. §5 initiator pendings: attempt-spend exposure
stated in the accounting (on-path capability, hiss-foreclosed mitigation),
precondition tightened to length-correct + index-matching + mac1-valid, and
re-arm-on-failure ruled in. §8 swap-cut qualified ("once the replacement
completes on both sides"; lost-msg2 one-way-dark window). §11 peer-restart
gains the ACK'd-loss statement (silent, confirmed, undetectable). §5
membership-oracle acceptance restated for v2 (liveness oracle,
`Internal`/`Claimed` discriminator) and flagged [MAINTAINER]. §4's
stage-0/durable-state warning extended to claimed static + source +
`sender_index`. §7 gains the endpoint-state-ceilings table (mid-state key
material and the 1024-provider-handle call-out; guard composition;
unbounded connection count with the ≈ 297 KB receive-buffer term). §5
pacing re-scoped per known static with the guard-entry counter and the
minimum-20 ms-spacing mechanism. §5's continuation restated as a numbered
list (tag → guard → pacing; record on full admission only; the
silent-drop-connection-untouched clause covering all three). NITs: §5
no-amplification recorded; §7 states the routes-by-index-but-fails-to-open
invariant; §10 says what `slither::policy` traces now.

**Group 9 — mechanical/definitional.** §6: `Timeout(None)` defined (both
cores, `L` applies to `Some`), and the lateness quote fixed to
`Timeout(Option<Instant>)`; teardown-precedes-keepalive (inverted, with the
`endpoint.rs:904–925` reconciliation and the reachable-at-25 s note);
`core::Endpoint<I: Identity>` generic (with the `I::Provider` mid-state-map
rationale); synchronous sealing stated (plan-seal-commit inside the
mutating call, protecting `on_pto` idempotency); the no-blocking invariant
stated with accessors as synchronous shared-cell reads; `Retired` stated as
a MUST (same drain, delivered before bookkeeping release); `session_id()`
defined. §9: `send_unreliable` shares the seq space, `Recovery` gains the
allocate-without-tracking primitive, interleaving test added; the lateness
bound applied by reference only (single home in §6). §2: `§9.7` row added;
v1 §8/§10 disposed (superseded by §11); the §6 row names the swap-cut and
epoch-death amendments; the §9–§9.3 row carves out the `ack_delay` clock.
§8: round-2 item 4 marked resolved with the `datagram.rs` citation
(`HandshakeError::DecryptionFailed`); `PERSISTENT_KEEPALIVE`
reclassification stated. §4/§7/§8: placeholder-constant draft-notes on
`INTRO_QUEUE_CAP`/`INTRO_MAX_PER_SOURCE`/`INTRO_TTL`,
`TS_GUARD_ORPHAN_CAP`, and `RECV_BUFFER`. §7: pin-on-claimed carve-out.
§5: DoS row-2 reworded ("unprobed (or drops it unprobed)"); pacing
severability note carried. §10/§4: no `Superseded`. Appendix A:
codegen-vs-runtime-accessor distinction for `next_counter()`. Appendix B:
handshake.rs functions ruled replaced and the four tests reclassified to
harness-rewrite [MAINTAINER]; `admit`/`next_packet` kept as thin wrappers;
recovery test count corrected to ten.

**[MAINTAINER] flags.** The original eight are carried: §1 no-fallback, §3
one-connection-per-static, §4 names-final, §4 dedup-key-supersedes-TODO, §6
two-cores, §7 eviction consequence, §8 swap cut, §11 peer-restart. Five are
added per the resolutions: §4 queue-DoS posture (Group 3), §4 `Superseded`
removal (Group 4), §7 guard-eviction mitigations (Group 8), §5
membership-oracle restatement (Group 8), and Appendix B
handshake.rs-replaced (Group 9). Round 3 adds one: §11
accept-re-home / deferred accept-commitment (F5). **Fourteen in total.**

## Round-3 changes applied *(non-normative index for the final verification)*

The five fixes of `round3-fixes.md`, with the sections they landed in.
Where a fix supersedes a round-2 ruling recorded above, the entry below is
authoritative: F1 supersedes Group 2's pre-`ss` tie-break timing (the
loser-resolves-at-`Install` half of Group 2 stands), F2 supersedes Group
8's re-arm ruling, F3 supersedes Group 3/4's consumed-chains-exempt cap
accounting, and F5 supersedes Group 7's `AcceptError::Expired`.

**F1 — the tie-break is gated post-`ss` (BLOCKER).** §5 *Simultaneous
open*: the tie-break runs only on an authenticated inbound — the
continuation runs `es` → `ss`, an `ss` failure is a silent drop with the
pending untouched, so a forgery dies at `ss` and cannot cancel a pending;
the winner drops the authenticated inbound, the loser cancels its pending
post-`ss` and responds. The internal continuation renumbered — tag → guard
→ pacing → tie-break → admit — with steps 1–3 leaving any pending
untouched and the winner-side drop silent, traced, and record-free.
"Decided at the claimed-static match, before the second DH" deleted; §5
steps 3–4 route a known-static match to the continuation (tie-break
applied inside it) instead of acting on the claimed static. Cost stated
(2 DH on the inbound — the honest simultaneous-open cost); the DoS table's
forged-known-static row now also prices a forged simultaneous-open claim —
no new row. §2's v1 §5 row names the post-`ss` tie-break. Appendix B's
tie-break obligation gains the forgery-cannot-cancel assertion.

**F2 — the immediate re-arm on failed completion is removed (MAJOR).** §5
*Initiator pendings*: a failed completion spends the attempt and the
**next scheduled retransmit** (`RETRANSMIT_BASE` + jitter) refreshes it —
at most one fresh initiation per interval regardless of forged-msg2
volume (v1's behaviour, restored). §5's DoS accounting keeps the exposure
documentation (on-path / index-observing; full mitigation foreclosed by
hiss's consuming state machines) and drops the one-round-trip claim. §2's
v1 §5 row updated; Appendix B's re-arm obligation becomes
one-attempt-per-interval.

**F3 — the per-source cap counts consumed + unconsumed chains (MAJOR).**
§4: `INTRO_MAX_PER_SOURCE = 4` counts the sum of unconsumed stage-0
entries and consumed chains per source (per /64 on IPv6);
`read_identity()` is net-zero for its source's count; eviction —
per-source replacement and overflow alike — stays on the unconsumed tier
only, and an arrival from a source whose whole allowance is consumed
chains is dropped. The accounting bullet and honesty clause corrected: a
single source is bounded to 4 chains total, a distributed attacker needs
one IP per 4 chains, and the application's probe policy governs how many
it authenticates. Appendix B's per-source-cap obligation annotated.

**F4 — ACKs derive from the masked set (MAJOR).** §8: ACK-range
construction operates on the effective acknowledged set — the window
bitmap AND NOT `shed_mask`; every ACK field derives from it, `largest` is
the greatest marked-and-not-shed counter, ranges are computed over the
masked bitmap, and `AckFrame::from_window` (or its equivalent) takes the
masked snapshot, never the raw window. §9's interplay rules gain the
matching bullet; §2's §9–§9.3 carve-out restated (every field, `largest`
included). Appendix B gains the reachable-in-steady-state obligation:
shed the window's greatest counter, assert it is absent from the emitted
ACK and that `largest` steps down to the greatest unshed marked counter.

**F5 — `accept()` re-homes to the freshest parked initiation (MAJOR)
[MAINTAINER].** §4 gains the re-home rule: fast path when the `Proven`'s
own initiation is still the freshest (no re-home, no extra DH); otherwise
`es` + `ss` on the freshest parked initiation for the proven static
(same-static verification, guard admission, msg2 for that initiation);
`AcceptError::Stale` when nothing is parked, and `AcceptError::Expired`
removed — a chain's age never fails an `accept()`. §4's expiry,
resolve-promptly, errors, and cost-footnote clauses updated to match;
§6's `accept()` comment and §10's taxonomy updated (`Stale` in, `Expired`
out); §5 gains the cost note (inspect `es` + `ss` plus fresh
`es` + `ss` + `ee` + `se`; app-driven, post-authentication, pacing-gated —
not amplification); §7's guard-admission sentence names the re-home
admission site; §11 gains the [MAINTAINER] known-behaviour clause with
the deferred accept-commitment model. `INTRO_TTL = 15 s` stays on the
unconsumed tier.

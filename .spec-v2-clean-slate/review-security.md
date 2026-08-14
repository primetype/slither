# Security review — SPEC-DRAFT-v1.md (wire version 2)

> Adversarial security review, 2026/08/13. Reviewer lane: **security only**.
> Correctness-of-mechanism and buildability are other reviewers' lanes except
> where a bug is a security bug (SEC-4, SEC-5, SEC-7 straddle the line and are
> flagged as such). Threat model as briefed: off-path spoofing injector;
> on-path observe/drop/reorder/modify; a malicious but successfully
> authenticated peer (post-`ss`); restarting and IP-sharing peers. The
> Noise/hiss core is trusted; everything slither builds around it is not.
> Deliberate accepted tradeoffs recorded in `architecture.md` are not
> re-litigated; divergences of the spec from its own design record are.

## Summary

| Severity | Count | IDs |
|---|---|---|
| BLOCKER | 1 | SEC-1 |
| MAJOR | 5 | SEC-2, SEC-3, SEC-4, SEC-5, SEC-6 |
| MINOR | 5 | SEC-7, SEC-8, SEC-9, SEC-10, SEC-11 |
| NIT | 2 | SEC-12, SEC-13 |

**Wire blast radius: zero.** Every fix below is a rule change inside the
sealed layer or in endpoint routing. **No finding moves a wire byte, a header
layout, a frame type, a frame encoding, or a ratified constant value**, with
one conditional: SEC-1's *preferred* fix (a byte budget) is wire-free, but if
the maintainer instead wants QUIC-style explicit path validation that needs a
new frame type from the reserved varint space — that is the only route in this
review that touches the wire, and it is the alternative, not the
recommendation.

---

## Confirmed defects

### SEC-1 — BLOCKER — third-party reflection via unvalidated roaming/anchoring, with no anti-amplification budget

**Sections:** §7.3 (roaming), §6.9 ("No amplification"), §14.5–§14.6 (cwnd
gate and reset seams), §12.4 (pure ACKs bypass cwnd), §13.4 (PTO probes
exempt), §5.5/§6.6 (msg1-source anchoring).

**The attack.** A malicious but successfully authenticated peer holds a live
session with us. It seals ordinary Data packets and emits them with the source
address spoofed to a victim `V`. Each such packet is authenticated and
window-fresh, so §7.3 moves our endpoint to `V` — one packet, no confirmation,
no return-routability check of any kind. From that instant every byte the
connection produces goes to `V`: application STREAM data, DATAGRAMs, credit
frames, ACKs, PTO probes, keepalives. The attacker then continues to spoof `V`
as its source and feeds us (a) fresh counters, which keep `last_recv`
advancing so liveness never reaps the session, and (b) **forged ACK frames**
covering the counters we are sending to `V` — trivially guessable, because
counters are sequential and in cleartext, and §12.5 only rejects an ACK whose
`largest` exceeds the highest counter *we have sealed*. Those forged ACKs
drain `bytes_in_flight`, grow `cwnd` through slow start (cwnd grows by bytes
newly acknowledged), and reset `pto_count`. The result is an unbounded,
self-accelerating flood at `V`, sourced from **us**, with the attacker's
identity nowhere on the victim's wire and the attacker paying one small packet
per RTT.

The `cwnd = INITIAL_WINDOW` reset on roam (§14.6) is not a mitigation: it is a
congestion-control decision, not an anti-amplification budget, and it is
defeated three ways. First, forged ACKs re-grow it. Second, §14.5 exempts pure
ACKs, CLOSE, and keepalives from the window entirely, and §13.4 exempts PTO
probes — so an unbounded-in-principle share of our output is not gated at all.
Third, §12.4 owes an ACK *immediately* on out-of-order arrival, so an attacker
sending ~30-byte sealed packets with deliberately gapped counters forces one
ACK each; a fused-window ACK carrying up to `MAX_ACK_RANGES` = 64 ranges is
several hundred bytes to ~1 KB. That alone is a cwnd-exempt reflector with a
per-packet amplification factor of roughly 10–30×, before any application data
is considered.

The same defect exists at establishment, not only at roam: §7.3 says an
accepted initiation "anchors" a session at its msg1 source, and §5.5/§6.6 send
msg2 and then all session traffic there. A key-holding peer that spoofs `V` as
its msg1 source gets the session anchored at `V` immediately. (The msg2 itself
is fine — 108 B against a 197 B stimulus — the problem is everything after
it.)

**Why this is a spec defect and not an accepted tradeoff.** The architecture
record carries roaming as "authenticated-fresh-marked only" from v1, where the
data path was WireGuard-shaped: no congestion control, no peer-driven rate
signal, no reverse-path ACK obligation. §14 imports QUIC's sending machinery —
an engine whose output rate is a function of *peer-supplied feedback* — but
does not import QUIC's counterpart control (RFC 9000 §8.2/§9.3: an endpoint
MUST NOT send more than three times the bytes received on an unvalidated
path, and MUST validate a new path before migrating full send rate onto it).
Neither `architecture.md` §B.12 nor §C mentions path validation or an
amplification budget; the omission is not a recorded decision, it is a gap.
§6.9's flat claim — "**No amplification.** … no path in this specification
emits bytes in response to an unauthenticated packet" — is true only in the
narrow sense that the *triggering* packet is authenticated; the *recipient* of
the emitted bytes never authenticated anything, which is the property that
matters for reflection.

**Fix (wire-free, recommended).** Add an anti-amplification budget keyed to
the current endpoint address, armed whenever the endpoint address changes
(roam) or is first anchored from a peer-supplied msg1 source:

- while an address is **unvalidated**, total bytes sent to it MUST NOT exceed
  `AMPLIFICATION_FACTOR` (3) × total bytes *received from* it on that session;
- the budget binds **all** output, explicitly including the §14.5 and §13.4
  exemptions (pure ACKs, CLOSE, keepalives, PTO probes) — those exemptions are
  scoped to the congestion window, not to this budget;
- an address becomes **validated** when it has been the source of at least
  `N` authenticated, window-fresh packets spread over at least one RTT — or,
  minimally and sufficiently for the reflection property, never: the 3×
  ratio alone caps amplification at the same factor QUIC accepts and forces
  the attacker to pay a third of its own flood, which removes the incentive
  entirely.
- Additionally: an initiator's session SHOULD anchor at the **dialled**
  address rather than the msg1 source when it dialled (it already does,
  §5.5 step 4 — state the responder-side counterpart).

**Alternative (moves the wire).** Explicit path validation: a
PATH_CHALLENGE/PATH_RESPONSE pair from the reserved frame space, roam
deferred until the echo returns, old address kept sending in the interim.
This is strictly stronger and strictly more expensive; it adds two frame
types and a fourth reset seam. Recommended only if the maintainer wants
QUIC-faithful migration semantics.

**Wire impact:** none for the recommended fix (a send-side budget and one new
non-wire constant). The alternative adds two frame types from the reserved
varint space.

---

### SEC-2 — MAJOR — credit-as-backpressure does not actually bound receive memory

**Sections:** §10.6 (the ruling and its supporting check), §9.5 (STREAM
reassembly), §16.8 (bounded channels, non-blocking), §17.5 (state ceilings).
`architecture.md` D.11 explicitly asks a reviewer to confirm this; the answer
is **no**, in two independent places.

**(a) Reassembly metadata is O(frames), not O(credit).** §10.6's supporting
check reads "no **non-stream, non-datagram** frame can force unbounded
buffering" — it excludes by construction exactly the two frame families that
can. Credit accounting is by offset high-water mark (§10.7), which correctly
bounds the *span* a receiver must cover: 1 MiB at the connection level. It
does not bound the *number of stored discontiguous ranges* inside that span.
An authenticated peer sends one-byte STREAM frames at offsets 0, 2, 4, 6, …
Each consumes 1–2 bytes of credit and creates a permanent hole. Within the
1 MiB connection window that is up to ~512 000 stored ranges; at the 40–100 B
per entry a `BTreeMap`-of-chunks reassembler costs, one peer converts its 1 MiB
advertised commitment into 25–50 MB of our memory — a 25–50× inflation of the
number §17.5 tells an application to size its accept policy against, and
§17.5 declares established connections "application-governed — unbounded by
the protocol". The peer pays ~30 B of wire per hole.

**(b) Delivered-but-unconsumed data escapes the accounting.** §10.3 makes
credit advance on *application consumption*, which is correct. But §16.8
delivers `ConnEvent::MessageReceived(Vec<u8>)` and
`ConnEvent::DatagramReceived(Vec<u8>)` — owned buffers — over "a bounded
channel with a non-blocking policy". If those buffers leave the connection
core before the application reads them, then either (i) they sit in a channel
that is outside the credit ledger, in which case memory is bounded by the
channel depth × 1 MiB-per-fill and credit is no longer the commitment, or
(ii) the non-blocking policy drops them, in which case *reliably delivered
stream data is silently lost* — a correctness failure worse than the memory
one. The spec never says which, and never defines "consumption" precisely
enough to close it.

**Fix (wire-free).**
1. State a normative reassembly-memory mandate: a receiver's per-stream
   reassembly state MUST be O(advertised credit) and MUST NOT scale with the
   number of received frames — e.g. a span-allocated buffer plus a
   received-bitmap (1 MiB span ⇒ 1 MiB + 128 KiB), or an explicit cap on
   stored out-of-order ranges with a defined behaviour on exceeding it. Add
   the corresponding worst-case row to §17.5.
2. Define "consumption" as the application taking bytes out of the connection
   core, and state that no unbounded intermediate queue may exist between core
   and handle: message/datagram payloads stay accounted inside the core (or
   inside the flow-control ledger) until the handle takes them. Reliable data
   MUST NOT be droppable by the no-blocking policy of §16.8.

**Wire impact:** none.

---

### SEC-3 — MAJOR — an unauthenticated mac1-valid packet can clobber DH-paid, sometimes authenticated, staged state (own-bytes-on-consume regression)

**Sections:** §6.3 (own-bytes-on-consume; the dedup/supersession rule),
§6.5 step 3 (eager demotion "carrying its paid mid-state"), §6.6 step 3
(restart parking "carrying its paid post-`ss` mid-state").

**The hole.** §6.3 rules that "an entry is **unconsumed** until
`read_identity()`", that an unconsumed entry's bytes are transparently
replaced by any newer initiation from the same source address, and — the
ruled invariant — that once consumed, "an unauthenticated mac1-valid packet
cannot clobber DH-paid work". Two paths park entries that carry paid DH state
**without** `read_identity()` having been called:

- §6.5 step 3 demotes an eager-path packet to stage 0 *carrying its `es`
  mid-state*, tagged identity-already-read;
- §6.6 step 3 parks a restart initiation *carrying its post-`ss` mid-state* —
  i.e. state for a **proven** identity.

Both are unconsumed by §6.3's definition, therefore both are byte-replaceable.
mac1's key is public data (§4.3), the dedup key is the source `SocketAddr`
alone, and nothing on this path requires a return route, so an off-path
attacker who can spoof the peer's 4-tuple (or simply shares its IP) mints a
mac1-valid garbage initiation and replaces the bytes of an entry whose cached
mid-state says "this is peer P, proven". The staged verbs then "return cached
results at 0 incremental DH" — so `read_identity()` and `authenticate()`
report **P's proven static for an initiation P never sent**. Whether
`accept()` then completes `ee`/`se` over the cached mid-state (installing a
session attributed to P at an attacker-chosen anchor address — see SEC-1) or
over the replaced bytes (in which case the identity shown to the application
was for different bytes), the application's authorisation decision is made
against state the attacker controls. This is strictly cheaper than the
exposure §17.1's honesty clause already accepts, which requires the attacker
to hold a *genuine* replayable msg1.

**Fix (wire-free).** Extend own-bytes to any entry carrying paid state: an
entry that carries a mid-state (eager-demoted or restart-parked) is
**consumed** for supersession purposes — its bytes and `IntroId` are frozen,
a later initiation from the same source parks as a new entry subject to the
per-source cap, and the accessors never straddle two initiations. Equivalently:
never carry a mid-state on a byte-replaceable entry — drop the mid-state on
replacement and re-pay. State explicitly which of the two, because the DH-cost
table (§6.1's footnote †) depends on it.

**Wire impact:** none.

---

### SEC-4 — MAJOR — the continuation's flag routing has no case for "outbound pending, no live connection", so simultaneous open bypasses the tie-break

**Sections:** §6.6 step 3, §5.4 (responder rule table), §6.5 step 3 (known
statics = established connections **∪ pending outbound remotes**), §6.7
(tie-break), §16.1 (one connection per static).

**The hole.** §6.5 routes an inbound initiation into the internal continuation
whenever the claimed static is in the *known-static* set, which explicitly
includes peers we hold an in-flight outbound pending to. §6.6 step 3 then
routes on the decrypted CONTINUATION flag with exactly two arms: `1` ⇒
proceed, `0` ⇒ "**the peer restarted**: the matched connection tears down". In
simultaneous open there **is no matched connection** — only a pending — and
both sides necessarily send CONTINUATION = 0 (under §16.1 you cannot hold a
live connection to a static you are dialling, so a fresh `connect()` is the
only thing in flight). §5.4's responder table says CONTINUATION = 0 with "no
local state for the static" is "the ordinary fresh accept", which contradicts
§6.5's routing of that same packet into the continuation, and the continuation
has no arm for it.

Read literally, the authenticated inbound of a simultaneous open takes the
restart arm (tearing down a connection that does not exist) or falls off the
end of the routing; either way it never reaches step 5, **the tie-break never
runs**, and both sides end with an outbound pending plus a staged `Intro` for
the same static. §6.7 exists precisely because "whichever completes first"
installs different key sets on the two sides and goes mutually dark for 15 s;
this hole re-opens that, and it additionally makes it possible for the
application to `accept()` an `Intro` for a static its own `connect()` is
completing, which is the one-connection-per-static invariant §16.1 calls "the
routing keystone".

This is a routing/mechanism bug with a security consequence (it defeats the
control installed for the SPEC-v2 tie-break BLOCKER), so it overlaps the
protocol-fidelity lane; reported here because of what it defeats.

**Fix (wire-free).** §6.6 step 3 must switch on three local states, not two:
**live connection** (CONTINUATION = 1 ⇒ proceed; CONTINUATION = 0 ⇒ the
restart branch, per SEC-6's deferral); **outbound pending only** (either flag
⇒ skip the restart branch, fall through to pacing and the §6.7 tie-break);
**neither** (⇒ the staged path, per §5.4 row 4 — and reconcile §6.5's
known-static definition so this case cannot enter the continuation at all).

**Wire impact:** none.

---

### SEC-5 — MAJOR — `Replaced` teardown fires pre-decision, on an initiation an on-path attacker can withhold and inject at will

**Sections:** §6.6 step 3, §5.4 (responder rule), §15.4 (teardown matrix),
§17.1 (honesty clause — whose stated worst case is now wrong).

**The attack.** §6.6 step 3 tears the live connection down (`Replaced`,
nothing transmitted) **before** the initiation is surfaced as an `Intro` and
long before the application decides anything. The only thing gating that
teardown is the timestamp guard at step 2, which admits any strictly-greater
timestamp. A genuine CONTINUATION = 0 retransmit that never reached us has, by
§5.5's fresh-initiation rule, a timestamp strictly greater than the one we
admitted. So: an on-path attacker observes the initial handshake, **drops one
retransmit** (say the third, ts = T3) while the responder establishes on the
second (guard = T2), then injects the withheld packet at a moment of its
choosing. Guard admits (T3 > T2), flags say CONTINUATION = 0, local state says
live connection ⇒ the working connection is destroyed and the responder's
application is handed a fresh `Intro` requiring a new accept decision (a human,
in the motivating use case). The initiator, which still believes it has a live
connection, discovers nothing until `DEAD_TIMEOUT`. The stored packet stays
usable until the guard advances past T3 — i.e. up to the first accepted rekey,
≈ `REKEY_AGE` = 120 s after capture.

Compared with the attacker's baseline capability (sustained dropping kills the
connection at 15 s and only while it stays on path), this buys a *one-shot,
delayed, off-path-at-use-time, application-visible* teardown, and it converts
§17.1's honesty clause from true to false: that clause reasons that the
observable consequence of a re-admissible genuine initiation is "a spurious
`Intro`/`Connection` … it dies at 15 s liveness and leaks nothing". Under the
new CONTINUATION routing the observable consequence is the **destruction of a
live connection**, which the clause was never ruled against.

Second, honest-operation cost of the same rule: if the application *rejects*
the restarted peer's fresh `Intro` (policy changed, human declines), the
working connection has already been destroyed for nothing, and by §17.1's
no-orphan-on-reject nothing was even recorded.

**Fix (wire-free).** Defer the teardown to the decision point: on
CONTINUATION = 0 from a static with a live connection, do **not** tear down at
step 3. Park the initiation as a fresh `Intro` (per SEC-3, as a consumed,
byte-frozen entry) and leave the existing connection running; tear it down
with `ConnectionLost::Replaced` **at `accept()`**, as the act that installs the
replacement — which is also where §16.1's one-connection-per-static must be
enforced anyway. A withheld replay then costs an unaccepted `Intro` and
nothing else; a genuinely restarted peer's zombie connection dies at
`accept()` or at liveness 15 s later, which is the same outcome the matrix
already documents for the peer side. Update §17.1's honesty clause to match.

**Wire impact:** none. (The flags, their semantics, and the payload bytes are
untouched; only *when* the teardown executes moves.)

---

### SEC-6 — MAJOR — the liveness anchor, as written, never fires for a continuously-sending application, and is defeatable by configuration

**Sections:** §7.4 (the ruled property), §7.5 (`DEAD_TIMEOUT`,
`PERSISTENT_KEEPALIVE`), §15.4 (teardown matrix row 1).

**The defect.** §7.5 states: "Liveness (`DEAD_TIMEOUT`) is measured **from the
last (marking) send**: sent-but-nothing-received for 15 s ⇒ the session is
dead", and §7.4 restates it as "a partitioned connection dies exactly
`DEAD_TIMEOUT` after **its last fresh send**". Taken literally — and it is
normative text — a marking send *restarts* the 15 s. An application that
writes to a stream every second into a black hole therefore resets the death
clock every second and the session **never dies**: `DEAD_TIMEOUT` is declared
"the only idle killer", so the connection is immortal. That is precisely
backwards from the intent, and it is the common case, not a corner: bulk
senders and periodic-telemetry senders both hit it.

The intended rule is WireGuard's — death at 15 s after the *first* marking
send that follows the last authenticated receive — and the whole `seal` /
`seal_quiet` split only makes sense under that reading. The spec's phrasing
expresses the other one.

Two further consequences of the same anchor:

- **Config-defeatable liveness.** `PERSISTENT_KEEPALIVE` is a per-connection
  `Option<Duration>` chosen by the application, unvalidated, and the keepalive
  is sealed via `seal` (marking). Any interval below `DEAD_TIMEOUT` — e.g. a
  5 s NAT-holding value, entirely plausible — makes every partitioned
  connection immortal under the literal reading, holding its index, its guard
  pin, its 1 MiB credit commitment, and its stream state indefinitely.
- It interacts with SEC-1: an immortal session is what lets a reflection
  target stay pointed at indefinitely.

**Fix (wire-free).** Restate §7.4/§7.5 as: the connection is dead when
`now − last_authenticated_recv > DEAD_TIMEOUT` **and** at least one marking
send has occurred since `last_authenticated_recv`. Equivalently, arm the
liveness deadline on the first marking send after a receive and do not re-arm
it on subsequent marking sends. Additionally either reject
`PERSISTENT_KEEPALIVE < DEAD_TIMEOUT` at the handle or exclude persistent
keepalives from the marking set (the former is simpler and preserves the
"application intent only" story).

**Wire impact:** none. `DEAD_TIMEOUT` = 15 s and the `seal`/`seal_quiet`
partition are unchanged; only the anchor's definition is corrected.

---

### SEC-7 — MINOR — the closing state's CLOSE reply is an unauthenticated-triggered emitter, aimed at an attacker-chosen address

**Sections:** §15.2 (local close / linger), §6.9 ("no path … emits bytes in
response to an unauthenticated packet"), §15.1.

**The hole.** §15.2 says a closing connection retains "**only** the seal
capability (all stream, flow-control, recovery, and congestion state may drop
immediately)" and "answer inbound packets from the peer with at most one CLOSE
per second". Seal-only means no open capability and no replay window, so
"inbound packets from the peer" cannot mean *authenticated* packets — it can
only mean "packets that routed to this connection by `receiver_index`". The
index is a 32-bit cleartext field, observable by anyone on path at any earlier
moment and guessable off-path. So during the 5 s linger, a 30-byte garbage
datagram bearing the right index elicits a sealed CLOSE (≈ 30 B + reason,
up to ~300 B) — a small but real amplification, and the spec does not say
where the reply goes: if it goes to the *triggering packet's* source, an
attacker aims it at any victim. Either way it contradicts §6.9's stated
invariant, which is load-bearing for the whole no-amplification story.

**Fix (wire-free).** State that the linger retains the **open** capability and
the replay window as well as the seal capability, that a reply is emitted only
for a packet that authenticates and is window-fresh, and that the reply is
sent to the session's endpoint address (never to the triggering packet's
source — the closing state does not roam). Retaining open + a 256-byte window
for 5 s is cheap; retaining only seal is what creates the hole.

**Wire impact:** none.

---

### SEC-8 — MINOR — RESET_STREAM's `final_size` is an attacker-controlled 62-bit value with no flow-control check in its own error list

**Sections:** §8.4 (RESET_STREAM semantic violations), §9.6 (the true-up),
§10.1/§10.5 (the limits and the violation rule).

**The hole.** §9.6 has the receiver count a peer-supplied `final_size` against
`MAX_DATA` consumption "exactly as if the bytes had arrived", and `final_size`
is a varint up to 2⁶² − 1 that requires no bytes on the wire to assert.
§8.4's RESET_STREAM entry enumerates its semantic violations as exactly
`STREAM_STATE_ERROR` and `FINAL_SIZE_ERROR` — **`FLOW_CONTROL_ERROR` is
absent**. §10.1 and §10.5 do bound the connection-level sum and do make
exceeding it a violation, so the rule exists globally; but §8.4 is written as
the per-frame error catalogue, and an implementer working from it accepts a
`final_size` of 2⁶² and adds it to a consumption counter. Two consequences:
the counter is driven past the limit with zero wire cost, and a naive
`consumed + final_size > limit` check on `u64` overflows for a handful of
streams, wrapping the comparison and silently re-opening the window.

**Fix (wire-free).** Add to §8.4's RESET_STREAM row and to §9.6: a
`final_size` that would push stream-level or connection-level consumption
above the advertised limit is `FLOW_CONTROL_ERROR`, checked **before** the
true-up is applied, with checked/saturating arithmetic mandated. Same
treatment for a STREAM frame's `offset + length` (already covered by §9.5's
credit rule, but state the ordering). Raise this to MAJOR if §8.4's per-frame
lists are intended to be exhaustive rather than illustrative — the spec should
say which.

**Wire impact:** none.

---

### SEC-9 — MINOR — reset streams' credit is never released, so connection-level flow control can wedge permanently

**Sections:** §9.6, §10.3 (consumption-driven re-advertisement), §9.7 (GC).

**The hole.** §9.6 has the receiver charge a reset stream's `final_size`
against connection-level consumption, and the receive half then "discards its
reassembly buffer" and closes when the application observes the reset. §10.3
makes credit advance only when "**application consumption** has advanced" the
level — and the application never consumes the trued-up bytes of a reset
stream; it consumes a `ReadError::Reset`. Nothing in §10.3 or §9.7 says the
reset's `final_size` counts as consumed for re-advertisement purposes.
Four resets of 256 KiB streams therefore burn the whole 1 MiB
`INITIAL_MAX_DATA` permanently: the connection stays alive, passes liveness,
and can never carry another byte from that peer.

The damage falls on the *sender* (it is the one that runs out of credit), so a
malicious peer only wedges itself — this is why it is MINOR and not higher.
But it is a live-connection deadlock reachable in honest operation by any
application that cancels streams, which is the ordinary use of RESET_STREAM.

**Fix (wire-free).** State in §9.6/§10.3 that a reset stream's `final_size`
counts as consumed for connection-level re-advertisement at the moment the
receive half is freed (§9.7), exactly as if the bytes had been read.

**Wire impact:** none.

---

### SEC-10 — MINOR — the eager path's `es` is uncapped per spoofed source, and re-home candidates are attacker-chosen

**Sections:** §6.5 step 3, §6.4 (re-home), §6.9 (the accounting table).

**The gap in the accounting, not in the design.** Two rows of §6.9 are
accurate but understate what the attacker controls:

- *"mac1-valid, src spoofed into the hint set, claimed static unknown — 1 DH."*
  True per packet, but the per-source cap and the queue cap are applied at the
  **demotion**, i.e. *after* the `es` is spent. So a single spoofed hint-set
  address buys an unbounded-rate 1-DH-per-packet oracle; the caps bound state,
  not work. §6.3's honesty clause discusses queue occupancy only, and the
  ceiling paragraph says 1 DH is "reachable without holding a hint-set
  address" — it should also say the rate is ungated. §19 does name cookies/mac2
  as the answer to "§6.5's hint-set spoof", so the posture is ruled; the
  *accounting text* is what is incomplete.
- *"A re-homed `accept()` … bounded by the per-source cap; not attacker
  amplification."* The four candidates the walk pays `es` + `ss` on are
  attacker-supplied whenever the attacker can spoof or share the peer's source
  IP: it fills that IP's four slots with mac1-valid garbage, so a legitimate
  `accept()` spends 8 DH walking rubbish and then returns
  `AcceptError::Stale`. That is both work the attacker chose and a denial of
  the accept itself, repeatable per attempt.

**Fix.** Documentation-level, plus one optional mechanism: state both
properties honestly in §6.9; optionally gate the eager `es` behind the same
per-source accounting the queue uses (charge the source's allowance *before*
the DH, refund on demotion), which converts the unbounded oracle into a
4-per-source-per-TTL budget at no correctness cost.

**Wire impact:** none.

---

### SEC-11 — MINOR — an idle-but-live session never rekeys, so one DH secret can cover months

**Sections:** §7.6 (`REKEY_AGE` as a payload-path consult), §7.5 (keepalive),
§7.7 (epoch ratchet).

**The observation.** `REKEY_AGE` triggers on "the first **payload** seal … past
a session age of 120 s", and explicitly exempts quiet control and inbound
opening. The keepalive is not payload. So a connection held open by the
keepalive dance — which §7.5 says "lives indefinitely" — performs **no DH
rekey ever**. The epoch ratchet does not compensate: it is counter-derived, and
a 10-second keepalive burns one counter per 10 s, so a single epoch spans
≈ 7.6 days and the ratchet is forward-rotation-only, not healing (§7.7 says so).
The consequence is that the post-compromise-secrecy interval the design
attributes to `REKEY_AGE` (120 s) is unbounded for idle sessions, which is a
material change to the security story of a long-lived idle tunnel.

This is a **carried ruling** (`architecture.md` B.16: "send-triggered,
payload-path-only consult, 2026/07/17 amendment carried"), so it is flagged
rather than argued. The cheapest correction, if the maintainer wants the
120 s bound to be real, is to let the keepalive count as a rekey trigger
(WireGuard's shape: any sent packet past Rekey-After-Time re-handshakes) —
the cost is one DH handshake per 120 s per idle connection.

**Wire impact:** none.

---

### SEC-12 — NIT — index unpredictability is security-load-bearing and the RNG is config-seedable

**Sections:** §16.6, §17.3, §5.5 step 3, §15.2.

`sender_index`/`receiver_index` unpredictability is what makes the
completion-attempt spend (§5.5) an on-path-only capability and what keeps
SEC-7's linger reply out of off-path reach. §16.6 gives the endpoint core one
seeded RNG whose 32-byte seed is "config-supplied for tests, OS entropy
otherwise" — a `Config` field an application can set in production. Add one
normative line: indices MUST be unpredictable to an off-path observer, and a
caller-supplied seed is a test-only facility (feature-gated, or documented as
security-relevant).

**Wire impact:** none.

---

### SEC-13 — NIT — §6.9's accounting table has two gaps

**Sections:** §6.9.

(a) The table prices no row for the **initiator-side forged msg2**, which
costs 2 DH (`ee` + `se`) per attempt — the prose beneath the table describes
the exposure but never prices it, so the "maximum cost of any single attacker
packet is 2 DH" ceiling reads as if handshake-responder rows were the only
2-DH paths. Add the row; the ceiling is unchanged and the table becomes
complete. (b) The "**No amplification**" paragraph needs the SEC-1
qualification: as written it claims a property the roaming/anchor path does
not have. Restate it as "no path emits bytes **to an address** that has not
authenticated", which is the property that actually matters and which SEC-1's
fix restores.

**Wire impact:** none.

---

## Couldn't rule out — needs analysis, not yet a defect

- **Adversarial (not merely lossy) fused-ACK behaviour.** §7.2/§12.2 keep the
  ACK fused to the 2048-bit replay window with newest-first truncation at 64
  ranges, and Appendix B gates the ruling on an ACK-**loss**-burst simulation.
  A malicious peer does not need loss: it can shape its own counter emission
  to hold ≥ 64 live ranges permanently, forcing every ACK we send to be
  maximal (§12.4 owes an ACK immediately on every gap-opening arrival) while
  starving the oldest ranges of ever being reported. The self-harm framing is
  probably right — the peer denies acknowledgment of its own packets — but the
  *reverse-path byte cost* is ours and is cwnd-exempt, which is what makes it
  a SEC-1 multiplier. **Ask:** extend the Appendix B obligation from
  "sustained ACK-loss bursts" to "adversarially shaped counter emission", and
  report the reverse-path bytes-per-inbound-byte ratio, not only the
  spurious-retransmit rate.
- **`app_limited` under a hostile peer.** §14.5 derives `app_limited` from
  "cwnd headroom existed but nothing was queued". A peer that manipulates ACK
  arrival timing controls when that predicate is evaluated. The obvious
  failure direction is benign (suppressed growth); the inverse — never setting
  `app_limited`, so a mostly-idle connection accrues window it did not earn —
  has not been analysed and feeds SEC-1. Needs one paragraph of reasoning
  in §14.5.
- **hiss `next_counter()` and AD/counter divergence.** The Data header is the
  AEAD AD and must be built before the seal (§3.4), which needs a counter
  accessor hiss 0.3.1 does not have (Appendix A.2, verified). If an
  implementation mirrors the counter in slither instead, divergence is
  fail-closed (a decrypt failure, never nonce reuse — hiss owns the send
  counter and the caller can never choose it, confirmed in
  `hiss/src/noise/datagram.rs` module docs). Safe *provided* the mirrored
  value is used only for the AD and never fed back to hiss. State that
  constraint normatively in §3.4 or A.2 so a workaround implementation cannot
  get it wrong.
- **`Retired` ordering versus index re-draw.** §16.4 makes `Retired` a MUST
  delivered before shell-side release, and §17.3 re-draws indices against both
  tables. If any shell released bookkeeping first, a re-drawn index could
  route to a half-torn-down connection. The spec forbids it and I found no
  attack; flagged only so the ordering test survives into Appendix B.

## Positive clearances

Subsystems examined adversarially and found sound as specified:

- **mac1 and the handshake DoS gate (§4, §3.2–3.3).** Verified before any
  curve work; keyed over public data with that fact stated honestly; tag over
  all preceding bytes including type/version/index. The 197 B → 108 B exchange
  is < 1× and, more importantly, msg2 is emitted only after a cryptographically
  valid, guard-fresh msg1 — no unauthenticated stimulus produces a handshake
  byte. The 2-DH-per-attacker-packet ceiling holds on every path I could
  construct (SEC-10 concerns the *rate*, a separate axis from the ceiling).
- **Counter / nonce / replay (§2.1, §3.4, §7.1, §7.2).** No path trusts an
  off-wire counter before the tag verifies: the replay check is strictly
  post-AEAD check-then-mark, `greatest` advances only on authenticated
  counters, and liveness and roaming both key on authenticated-and-marked.
  hiss ground truth confirms the two claims the design leans on — the receive
  half is stateless and opens any counter any number of times (so 100 % of
  replay protection is slither's, as stated), and a counter more than
  `MAX_EPOCH_JUMP` = 2 epochs ahead is refused **without deriving any key**
  (`hiss/src/noise/datagram.rs:74`, `:346-354`), so forged far-future counters
  can neither desynchronise the receiver nor buy work. No double-admission
  path found.
- **Cross-session replay at the rekey seam (§7.1, §7.6, §7.8, §17.3).** PN
  spaces reset to 0 per session but are separated by `receiver_index`, which
  is in the AD and re-drawn against both tables; the swap drops the old index
  and keys instantly. A packet from either session cannot be admitted in the
  other — wrong keys, and the header binds the index. The "routes by index but
  fails to open touches nothing" corollary closes the stale-traffic case.
- **The simultaneous-open tie-break's unforgeability (§6.7).** The SPEC-v2
  BLOCKER does **not** regress: the tie-break is reached only after `ss`, a
  match at `es` "selects the path but decides nothing", and an `ss` failure is
  a silent drop with the pending untouched. The comparison is over a proven
  static in one canonical encoding (§2.4), equal-length within a suite. Sound.
  (Its *reachability* is broken — SEC-4 — but not its unforgeability.)
- **ACK processing (§12.5).** Bounded intersecting against the in-flight set,
  never materialised; `range_count` capped at 64 structurally; `largest` above
  the highest sealed counter ignored whole; duplicate acknowledgment a no-op;
  ranges descending below zero structural. A wire-legal hostile ACK cannot
  force unbounded work. (It can lie — SEC-1 — but that is a path problem, not
  an ACK-processing problem.)
- **DATAGRAM queues (§11.3–11.5).** Count-bounded 64/64, drop-oldest with
  newest accepted, ≈ 75 KiB per queue, receiver-oversize unrepresentable by
  construction, drops counted. Nothing a peer can do here forces unbounded
  memory or work.
- **Stream-limit churn (§10.4, §9.7).** Cumulative counting with grants only
  on full closure bounds *concurrent* streams at the initial 32 + 128
  regardless of churn; the implicit-open rule is index-checked against the
  cumulative limit, so a frame naming stream 2⁶⁰ − 1 is a
  `STREAM_LIMIT_ERROR`, not an allocation. MAX_STREAMS churn is not a memory
  lever.
- **CLOSE forgery (§15.1–15.2).** Only the in-seal CLOSE exists, the cleartext
  close packet type stays dead, `0x04` is never emitted. Nothing
  unauthenticated can kill a connection. (The *linger reply* is separate —
  SEC-7.)
- **Frame-layer structural parsing (§8.2, §8.4).** Parse-then-apply with
  whole-packet rejection on any structural failure, no partial salvage, at
  most one extends-to-end frame in final position, everything bounded by
  `MAX_PLAINTEXT` = 1170. No parser-differential surface, no unknown-type
  tolerance to abuse.
- **Timestamp-guard write path (§17.1).** Writes are key-holder-only (all
  three admission points are post-`ss`), record-on-full-admission-only, LRU
  recency refreshed on admission only, and pinning protects every live
  connection's replacement protection from eviction. The honesty clause's
  *analysis* is now stale (SEC-5); the mechanism itself is sound.
- **Mid-state / provider-handle ceiling (§17.5).** The 1024-concurrent-handle
  worst case is reachable only through application-driven `read_identity()`
  calls; the endpoint-driven half (eager demotion, §6.5 step 3) is bounded at
  4 per hint-set source IP by the per-source cap, so an off-path spoofer
  cannot mint it. The disclosed ceiling is accurate.

## Verdicts on the flagged open items

**[OPEN] §6.4 — the re-home candidate-flag discard.** *Safe to delete the
discard clause; it is not a security control.* A CONTINUATION = 1 candidate is
authenticated post-`ss` — unforgeable by a non-key-holder — so discarding it
buys no security, while keeping it costs exactly the liveness failure the
writer identified (after a local restart every candidate carries
CONTINUATION = 1, so re-home always returns `Stale` while the fast path
accepts the identical initiation). Answering such a candidate with
CONTINUED = 0 is the honest, intended signal and is what lets the peer die
with `PeerRestarted` and reconnect fresh. **Recommended ruling:** delete the
candidate-flag discard and replace it with a *local-state* consistency check
evaluated at admission — if a live connection exists for the proven static,
`accept()` returns `AcceptError::AlreadyConnected` rather than admitting a
candidate. That is the case the discard was groping for, and it belongs to
§16.1, not to the flags. Wire impact: none.

**[OPEN] §15.2 — `LocallyClosed` versus a dedicated
`ConnectionLost::ProtocolViolation { code }`.** *Add the variant.* This is a
security-signal question, not an ergonomics one: "I closed" and "the peer's
misbehaviour forced me to close" are different facts and only the latter is
evidence about the peer. Collapsing them pushes the only peer-misbehaviour
signal onto a trace target, unavailable to an application doing peer
reputation, allow-list demotion, or alerting — and §18.1's own precedent is
that a security signal earns its own variant (`AuthError::HandshakeFailed` is
called out as "the only variant in the staged taxonomy that is a security
signal"). A closed taxonomy is a reason to close it *correctly*, not a reason
to omit. Wire impact: none — §15.3's registry already carries the code on the
wire; this is the local surface only.

**The unmarked reconciliation — roaming keeps `bytes_in_flight` with the kept
sent map (§13.6, §14.6).** *Internally consistent and safe as a recovery/CC
matter; it is the wrong place to look for the roaming risk.* Keeping the sent
map on roam is correct — ACKs for old-path packets still resolve,
`bytes_in_flight` stays consistent with the map it is derived from, and loss
detection and PTO continue undisturbed — and the stated consequence (a bounded
stall of at most one loss-detection/PTO cycle while `bytes_in_flight` exceeds
the fresh `INITIAL_WINDOW`) is accurate and benign. The alternative, dropping
the map, would be *worse*: it would strand in-flight frames outside both the
pending set and the loss tracker, which §16.7's plan-seal-commit invariant
exists to prevent. Two riders. (i) That stall is exactly what a hostile peer
erases with forged ACKs, so this reconciliation is an ingredient of SEC-1
rather than an independent problem — it is safe *once SEC-1 is fixed*, and the
anti-amplification budget must be applied on top of, not instead of, the kept
map. (ii) §13.1's `min_rtt = min(min_rtt, latest)` is monotone non-increasing
for the connection's whole life across arbitrarily many paths; a post-roam
`min_rtt` MUST be allowed to increase, or an old short path pins PTO too low
on a new long one and manufactures spurious probes (which are cwnd-exempt —
again SEC-1). That rider is MINOR-grade and folded here rather than given its
own ID because it is a tuning fault, not an attack.

## Overall verdict

**The security architecture is sound in its core and defective at one seam.**
The load-bearing parts are right: the DoS cost ladder is real and its 2-DH
ceiling holds, mac1 gates before any curve work, the staged accept's
"nothing durable keyed on a claim" discipline is maintained, the tie-break's
post-`ss` rule does not regress the SPEC-v2 BLOCKER, the replay window is
strictly post-AEAD, hiss's contracts are respected exactly as the §2.1 table
claims (verified against hiss source), and the handshake exchange is genuinely
sub-unity in amplification. The structural problem is that §14 imported QUIC's
*sending* machinery — an output engine whose rate is a function of
peer-supplied feedback — onto §7.3's WireGuard-shaped roaming, which moves that
engine's destination on a single authenticated packet with no
return-routability check and no anti-amplification budget, then exempts a large
share of the output (ACKs, CLOSE, keepalives, PTO probes) from the one window
that might have bounded it. That is SEC-1: a composition failure between two
individually reasonable inherited designs rather than a mistake in either, and
the one thing that must be ruled on before ratification. The remaining MAJORs
all close with local rule changes: one is the frame layer's memory story being
asserted rather than established (SEC-2 — and D.11 asked for exactly that
confirmation, which cannot be given as written), one is a liveness anchor that
says the opposite of what it means (SEC-6), and three are the new CONTINUATION
machinery not having been carried through the routing and supersession rules it
interacts with (SEC-3, SEC-4, SEC-5) — the expected shape of risk for the one
genuinely new mechanism in this wire break. **No finding requires a wire byte
or a ratified constant value to move**, so §1.3's one-time golden-vector
regeneration plan is unaffected by this review.

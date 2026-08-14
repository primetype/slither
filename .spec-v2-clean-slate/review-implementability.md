# Implementability review — SPEC-DRAFT-v1 (wire version 2)

> Reviewer lane: implementability only. Security and RFC-fidelity arguments are
> assumed to hold. Question asked of every clause: can a competent Rust engineer
> build this, on hiss 0.3.x + the declared Appendix A minor, in the §16 sans-io
> shape, without making a design decision the spec should have made?
>
> Sources: SPEC-DRAFT-v1.md (read in full), architecture.md, research-gaps.md,
> research-sansio-quinn.md, the hiss checkout at
> `/Users/nicolasdiprima/work/primetype/hiss` (datagram API verified directly),
> and the existing code at `/Users/nicolasdiprima/work/primetype/slither/src`.

**Tally: 0 BLOCKER · 6 MAJOR · 9 MINOR · 5 NIT.**

One verified non-finding worth recording up front: the spec's "header = AEAD
associated data" design rests on hiss's `DatagramSend::encrypt_next(&ad, ...)`
accepting caller-supplied AD and returning `(counter, bytes_written)` — both
verified present in hiss 0.3.1 (`src/noise/datagram.rs:184-205`). No fourth
undeclared hiss dependency exists; Appendix A's list of three is complete.

---

## The gating hiss dependencies (Appendix A) — can a v1 start today?

**Yes, for roughly 80 % of the build; no, for §6.** Assessment per dependency:

- **A.1 (split msg1 read)** is the one genuine external blocker, and only for
  the staged accept + eager continuation slice (§6, §5.6). The spec's no-fallback
  stance is *forced as specified*: the `ReadStyle::Verify` closure path exists in
  0.3.1 but yields a 3-DH accepted read, which the normative DH-cost table (§6.1)
  and the Appendix B DH-cost pins (1/2/4) rule out — an implementation on the
  closure would fail the spec's own tests. So §6 waits on the hiss minor.
  Everything else — wire codec, mac1, session layer, frame layer, streams, flow
  control, ACK/recovery/CC, CLOSE, the connection core, most of the endpoint
  core, and the shell — has no A.1 dependency and can start now; connection-core
  flow tests can establish sessions via the plain (non-staged) IK read in test
  harness code. Critical path: hiss ships the A.1 minor → §6 lands last.
- **A.2 (`next_counter()`)** does **not** block: the mirror-and-assert interim
  (slither tracks the counter in lockstep, `debug_assert_eq!` against the value
  `encrypt_next` returns) is *already shipped* in today's
  `src/session.rs::seal_inner` and is verifiably correct against hiss's
  strictly-monotonic contract. See IMP-7.
- **A.3 (`AsRef<[u8]> + Ord` bound)** does **not** block: slither-side
  where-clauses (`where C::PublicKey: AsRef<[u8]> + Ord`) compile against 0.3.1
  today and every shipped curve satisfies them. The hiss bound change is a
  cleanliness/guarantee matter, not a prerequisite. See IMP-8.
- No latent contradiction found: nowhere does the normative text assume A.1–A.3
  already exist; every use points at the appendix. The `[13]`/`[1]` payload
  non-item is correctly claimed as shipped (independently confirmed by
  research-gaps.md Fact 1 with file:line evidence).

---

## Findings

### IMP-1 · MAJOR · §15.2, §6.9 — the closing state's retained capabilities contradict the no-amplification invariant

§15.2 says a locally-closing connection "retain[s] only the seal capability
(all stream, flow-control, recovery, and congestion state may drop
immediately)" yet must "answer inbound packets **from the peer** with at most
one CLOSE per second". With only the seal half retained, "from the peer" can
only mean "routed by `receiver_index`" — which an unauthenticated forger who
has observed the cleartext index can satisfy, making the sealed CLOSE reply a
response to an unauthenticated packet. §6.9 states flatly that "no path in
this specification emits bytes in response to an unauthenticated packet". An
implementer cannot satisfy both clauses and must choose: (a) retain the open
half (recv cipher state + replay window) and reply only to authenticated
inbound, or (b) reply on index match and accept the (rate-bounded) exception
to §6.9. **Fix**: state the retained state set exactly — recommend "seal
capability, the receive cipher states, and the replay window; a reply is owed
only to an authenticated, window-marked inbound packet", and adjust the §15.2
"retain only" wording. **Purely internal** (no wire bytes; the reply packet is
identical either way). Overlaps the security lane, but it is flagged here
because the text as written is unimplementable-as-a-conjunction.

### IMP-2 · MAJOR · §16.9, §6.7, §9.1, §16.2 — early sends collide with tie-break-assigned stream parity

§16.9 makes pre-establishment stream opens "ordinary work": the core exists
from `connect()`, early opens/writes/messages land in ordinary stream state.
But a stream open must allocate a `StreamId`, whose opener bit (§9.1) is fixed
only at establishment: under simultaneous open, the dialler that loses the
tie-break (§6.7) becomes the *acceptor*, so any IDs pre-allocated with
initiator parity are wrong on the wire, and `SendStream::id()` (§16.2) is a
synchronous accessor apparently available before establishment. The spec never
addresses this intersection. An implementer is forced to invent one of: lazy
ID assignment at install (then `id()` must block/error pre-establishment);
provisional IDs renumbered at install (then the shell's per-`StreamId` waker
maps and any application-held IDs need a remap story); or forbidding
`connect()`-side opens before establishment (contradicting §16.9). **Fix**:
rule it — the natural rule is "stream IDs are assigned at establishment; before
it, handles hold core-internal indices and `id()` on an unestablished
connection's stream is defined (blocks, or returns the ID lazily post-install)".
**Purely internal** (any resolution produces the same on-wire IDs, since no
frame is transmitted before install).

### IMP-3 · MAJOR · §16.4, §9.8, §11.3/§11.5, §16.8 — the incoming-supply ownership model (push events vs claim semantics) is unresolved

Three receive surfaces are specified as *pull-with-claim* at the API level but
*push* at the core level, and the two models conflict:

1. **Messages vs `accept_uni()`** (§9.8): "a stream claimed by `accept_uni()`
   leaves message consideration; `recv_message()` surfaces the oldest
   fully-reassembled unclaimed uni stream; which mode consumes a given stream
   is the receiving application's choice." But `ConnEvent::MessageReceived(Vec<u8>)`
   (§16.4) is a spontaneous push: the core would have to convert a
   fully-reassembled *unclaimed* stream into a message (and free it, per §9.8)
   before knowing the application's choice — after which an `accept_uni()`-only
   application can never see that stream. The claim rule and the push event
   cannot both hold.
2. **Datagram receive queue** (§11.3): `DATAGRAM_RECV_QUEUE = 64`, drop-oldest,
   drops counted on `slither::frames` (§11.5). If `ConnEvent::DatagramReceived`
   pushes each datagram out of the core, the 64-bound queue can only be the
   shell's channel (§16.8) — but then the *core* cannot count or trace the
   drops, and the bound is a shell conformance detail the spec presents as core
   behaviour. Whose queue is it?
3. **Stream accept**: no core verb exists to claim a peer-opened stream at all
   (see IMP-4).

**Fix**: pick the pull model uniformly (the quinn shape, which the research
record already endorses): the core retains reassembled-but-unclaimed uni
streams and queued received datagrams; `ConnEvent` variants become *signals*
(`MessageReadable`, `DatagramReadable`, `StreamOpened{dir}`) and the core gains
claim verbs (`recv_message() -> Option<Vec<u8>>`, `recv_datagram() ->
Option<Vec<u8>>`, `accept(dir) -> Option<StreamId>`); the 64-datagram bound and
its drop counter live in the core. Also state the retention rule for
reassembled-never-claimed uni streams (they hold their stream-state and their
MAX_STREAMS credit until claimed — which is the natural backpressure). **Purely
internal.**

### IMP-4 · MAJOR · §16.4, §16.2, §10.4 — the ConnEvent set cannot wake every blocking shell verb

§16.8 mandates that a blocked stream verb parks its waker and "is woken by the
matching `ConnEvent`", but the event set is incomplete for the verbs §16.2
declares: (a) `StreamOpened { dir }` carries **no `StreamId`**, so the shell
cannot construct the `SendStream`/`RecvStream` handles that `accept_bi`/
`accept_uni` must return without a core claim verb the API listing does not
have; (b) `open_bi`/`open_uni`/`send_message` "wait for MAX_STREAMS allowance"
(§16.2, §10.4) but **no event announces allowance arrival** (a
`StreamsAvailable { dir }` on receipt of MAX_STREAMS is missing), so the shell
has no wake edge for those waiters. Two implementers would necessarily invent
different core surfaces here. **Fix**: add the id (or the claim verb, per
IMP-3) and a streams-allowance event; while there, state when
`StreamWritable` is emitted (on stream/connection credit arrival for a stream
with a blocked writer — currently only derivable). **Purely internal.**

### IMP-5 · MAJOR · §10.3, §9.6, §16.2 — connection-level credit accounting for unread bytes (reset, abandoned, discarded) is unspecified

§10.3 keys credit advance on "application consumption" ("an unread buffer
earns nothing"), and §9.6 trues up the *violation-checking* ledger to
`final_size` on RESET_STREAM. But nothing says whether unread bytes ever count
as **consumed for MAX_DATA re-grant purposes** in the three cases where the
application will never read them: a reset stream's discarded reassembly buffer
(§9.6), an abandoned `RecvStream` whose arrivals are discarded (§16.2 — the
spec explicitly says "credit never advances", without scoping the statement to
the stream level), and a reassembled-never-claimed stream freed at final size
(§9.7). If those bytes never re-earn connection credit, every such stream
permanently shrinks the connection window — a slow, spec-compliant march to a
total send stall (connection-death-by-credit-leak). If they do, the spec must
say when. **Fix**: one sentence — "when a receive half is freed or its buffer
discarded (reset observed, handle abandoned, final size reached with no
reader), its `final_size` counts as consumed at the connection level for the
§10.3 re-grant rule; stream-level credit is simply never re-granted." **Purely
internal** (affects when MAX_DATA frames are emitted, but any value emitted is
wire-legal).

### IMP-6 · MAJOR · §6.3, §6.5 step 3 — replace-with-newest vs the carried mid-state of an eager-demoted entry

An eager-demoted entry parks at stage 0 "carrying its paid mid-state, tagged
identity-already-read" (§6.5), and §6.3's dedup rule says an *unconsumed*
entry's bytes are transparently replaced by a newer same-source initiation
("accessors reflect the newest bytes at call time"). A demoted entry is
unconsumed until the app calls `read_identity()` — so it is replaceable while
carrying a mid-state derived from the *old* bytes. The spec does not say what
replacement does to the carried mid-state. In the common case the newer arrival
also takes the eager path and brings its own fresh mid-state (self-healing),
but if the hint set changed between the two arrivals the replacement parks
*without* one — and an implementation that keeps the stale cached claim would
have `read_identity()` report an identity that does not match the parked bytes,
and `authenticate()` would then run `ss` against a tail that no longer exists.
**Fix**: one sentence — "replacement replaces the entry wholesale: bytes,
TTL, and any carried mid-state (a replacement without its own mid-state clears
the identity-already-read tag; `read_identity()` then pays its 1 DH normally)."
**Purely internal.**

### IMP-7 · MINOR · Appendix A.2, §3.4 — say whether the counter mirror is a permitted interim

A.2 states the normative requirement behaviourally ("the header carries exactly
the counter the seal used") and frames the accessor as what "makes an
implementation not mirror hiss-owned state" — while the current production code
(`src/session.rs::seal_inner`) ships exactly the mirror-and-assert pattern and
is correct against hiss's monotonic contract. As written, an implementer cannot
tell whether v1 may ship on the mirror (behavioural reading: yes) or must wait
for the accessor (gating reading: no). This is the difference between A.2 being
on the critical path or not. **Fix**: one clause — either "the mirror-and-assert
interim is conformant until the accessor ships" or "implementations MUST NOT
mirror; A.2 gates". Recommend the former; the assert makes divergence
impossible to miss. **Purely internal.**

### IMP-8 · MINOR · Appendix A.3, §2.4 — the bound is obtainable today via slither-side where-clauses

`where C::PublicKey: AsRef<[u8]> + Ord` on slither's own generics compiles
against hiss 0.3.1 (extra bounds on associated types are ordinary Rust), and
every shipped curve satisfies both, producing byte-identical mac1/tie-break/
identity-map behaviour. A.3 is therefore an API-hygiene dependency (and a hiss
semver ruling), not an implementation gate; the spec's "gates on one hiss 0.3.x
minor carrying three additions" overstates it. **Fix**: note the where-clause
interim in A.3 so the critical path is visibly A.1 alone. **Purely internal.**

### IMP-9 · MINOR · Appendix A.1 — the Mid-state surface is named but not shaped

A.1 pins the important choices (owned tail, no lifetime, no re-supply) but not
the surface slither's §6 consumes: that the `Mid` state must expose the
**claimed static** (both `read_identity()`'s return and §6.5's eager
inspection need it), and that `complete()` must return the 13-byte payload
alongside the responder state (the guard and flag routing consume timestamp +
flags post-`ss`, §6.6 steps 2–3). Both are obvious to the co-designed hiss
side, but since Appendix A is the only written contract for the minor, the two
sentences are cheap insurance against a shipped API that almost-but-not-quite
fits. **Purely internal.**

### IMP-10 · MINOR · §8.5, §13.4 — no stream-scheduling rule for packet fill

§8.5 orders frame *types* within a packet and §13.4 orders probe content
("pending retransmittable frames oldest-first"), but nothing orders *which
streams'* pending ranges fill ordinary packets (round-robin, FIFO by write
time, lowest-ID-first?). Any choice is wire-legal and interoperable, but
fairness across streams is the product feature streams exist for, and two
implementations will observably differ (one stream can starve another for the
whole connection under a full window). **Fix**: name a rule (round-robin over
streams with pending data is one sentence and matches the no-HOL-blocking
pitch), or explicitly declare scheduling implementation-defined. **Purely
internal.**

### IMP-11 · MINOR · §16.2 — `finish()` and `close()` await semantics unstated

Both are `async` on the shell, and the spec never says what they resolve on.
`finish()`: on FIN queued, or on FIN sent, or on the send half reaching
`DataRecvd` (the `StreamFinished` event exists, so full-ACK-await is
representable)? `close()`: on CLOSE emitted, or on linger expiry (5 s later —
a material difference for callers)? Two implementations diverge on
user-visible behaviour. **Fix**: one sentence each; recommend `finish()`
resolves when the FIN is accepted into the send state (errors surface via
`WriteError`), `close()` resolves once CLOSE is sealed and the connection has
entered closing. **Purely internal.**

### IMP-12 · MINOR · §7.5 — persistent keepalive's trigger rule is not stated

"Persistent keepalive is a per-connection opt-in interval for NAT holding" —
but not whether the beacon fires on a fixed cadence or only when nothing has
been sent for the interval (WireGuard's actual rule), nor whether receiving
resets it. The timer is in the §16.5 table, so its arm/re-arm rule is core
behaviour a test must pin. **Fix**: adopt WG's rule explicitly ("if no marking
send has occurred for the interval, send a keepalive; re-arm from every marking
send"). **Purely internal.**

### IMP-13 · MINOR · §16.5 vs §17.1 — the orphan-aging deadline is missing from the endpoint core's deadline set

§17.1 mitigation (ii) requires timestamp-guard orphans to "age out on an
`INTRO_TTL`-scale timer", but §16.5 defines the endpoint core's announced
deadline as "the min over its pendings' retransmit/give-up deadlines and the
parked intros' expiries" — the orphan-aging deadline is absent from the
enumeration, so a shell driving exactly the announced `Timeout` would never
fire it. **Fix**: add orphan aging to the §16.5 endpoint deadline enumeration.
**Purely internal.**

### IMP-14 · MINOR · §14.5 — the `app_limited` derivation needs one concrete sentence

"Derived per ACK from 'cwnd headroom existed but nothing was queued' since the
acknowledged packet was sent" describes an intent, not a bookkeeping rule: an
implementer must invent what is sampled and when (a flag on each sent packet? a
timestamp of the last headroom-and-empty observation, compared against the
acked packet's send time?). Divergent choices change window growth, which the
Appendix B throughput obligation then measures. **Fix**: pin one mechanism
(quinn's is: an `app_limited` flag maintained by the send path, recorded onto
each sent packet, passed through on ack). **Purely internal.**

### IMP-15 · MINOR · §16.4, §6.2 — core-level staged-verb and accept-return signatures are only implied

The endpoint core lists its staged verbs as a comment ("read_identity /
authenticate / accept / reject, by IntroId") with no signatures, and the
"accept() is never followed by an Install" bullet implies core `accept(IntroId)`
returns a pre-installed `(ConnectionId, core::Connection)` by symmetry with
`connect` — but only implies it. Similarly `EstablishedSession` (the Install
payload) is never defined even in shape. Decidable by a careful reader;
worth two lines to remove the inference. **Purely internal.**

### IMP-16 · NIT · §7.6, §16.4 — `NeedsRekey` re-emission is undefined

If the connection emits `ToEndpoint::NeedsRekey` on *every* payload seal past
age 120 s, the endpoint must dedup against its in-flight pending (which the
one-pending-per-static invariant supports); if once per session, the core needs
a latch. Either works; say which. **Purely internal.**

### IMP-17 · NIT · §14.4 — whether the persistent-congestion PTO includes the backoff multiplier

"persistent_period = PTO × 3" — RFC 9002 §7.6.1 computes this from the base
PTO (no `2^pto_count` backoff). The spec's PTO is defined with backoff in
§13.3; one clause should pin the base-PTO reading. **Purely internal.**

### IMP-18 · NIT · §16.5 — `AckDelay`'s equal-deadline priority is unlisted

The priority list covers give-up/retransmit, loss/PTO, and teardown/keepalive
but not where `AckDelay` (or `PersistentKeepalive`) sit at an equal instant.
Almost certainly immaterial; the list claims to be normative, so complete it.
**Purely internal.**

### IMP-19 · NIT · Appendix B — the throughput obligation's "target rates" are undefined

"The flow-control and stream-limit initials sustain **the target rates** over
FlakyWire … compared against quinn's shipped defaults" names no number, so the
gate cannot fail objectively. Name a rate (or define it as "within X % of the
same topology under quinn's defaults"). **Non-normative appendix; internal.**

### IMP-20 · NIT · §8.4 — "not implicitly openable" in the MAX_STREAM_DATA error case

§9.2 says only STREAM/RESET_STREAM open streams, so "a stream not yet open and
not implicitly openable" (in MAX_STREAM_DATA's semantic-violation clause) has
no referent unless credit frames can open streams — they cannot. Reword to
"for a stream in a space the frame's receiver opens that the receiver has not
yet opened ⇒ STREAM_STATE_ERROR" (QUIC's rule). **Purely internal.**

---

## The two [OPEN] markers and the unmarked roaming reconciliation

- **[OPEN] §6.4 (re-home candidate-flag discard vs the we-restarted case)** —
  does not block. The spec states the believed-intended rule (discard applies
  to re-home only; the fast path proceeds per §5.4) precisely enough to
  implement as the default; the ruling flips one narrow branch and moves no
  wire byte. Decidable by the implementer, ratifiable later.
- **[OPEN] §15.2 (`LocallyClosed` vs a dedicated `ProtocolViolation` variant)**
  — does not block. Pure error-taxonomy surface; the stated default
  (`LocallyClosed` + trace detail) is implementable as written, and the
  alternative is an additive enum variant.
- **The unmarked reconciliation (§13.6/§14.6, roaming)** — cwnd resets while
  the sent map (and thus `bytes_in_flight`) is kept, so immediately after a
  roam `bytes_in_flight` may exceed the fresh window and the admission gate
  blocks new sends. The spec reconciles this deliberately ("a bounded stall of
  at most one loss-detection/PTO cycle") rather than zeroing `bytes_in_flight`
  or exempting post-roam sends. **Sound and implementable**: `bytes_in_flight`
  stays definitionally the sum over the kept map; PTO probes are gate-exempt so
  the path stays probeable; ACKs/losses drain the excess. No gap. (One test
  obligation worth adding: the post-roam stall resolves within one PTO cycle on
  the paused clock.)

---

## Clean areas (verified, no findings)

Packet grammar and all §2.3/§3.5 arithmetic (cross-checked: 197/108/14+16/1170/
1169 all consistent); mac1 construction; handshake payload layouts and the
continuation-flag matrix (§5.4 is a complete decision table); the varint codec,
frame layouts, parse-then-apply split, and every structural-error case (§8 is
codec-complete — golden vectors are writable from the spec alone given hiss);
the consolidated constants table (checked against every inline use — no
constant needed by the codec or timers is missing); the replay window, ACK
derivation/truncation/processing, and the delayed-ACK policy; RTT/loss/PTO
(§13 is RFC 9002-shaped and complete); the Controller trait wiring,
`bytes_in_flight` add/remove sites, gate exemptions, and both reset seams; the
rekey survival matrix and re-queue rule (§7.8 is exemplary — explicit lists,
nothing to infer); plan-seal-commit and synchronous sealing; the `Retired`/
`Install{initial}`/`HandshakeFailed` shell contracts; the RNG root/sub-seed
rule; §17's four global-state pieces; the error taxonomy (closed, every
variant homed); kernel-free paused-clock drivability (no hidden wall-clock,
IO, or `Send` dependency found; the one wall-clock read is behind the injected
clock service; both Appendix B validation obligations are runnable against the
described core, modulo IMP-19's undefined target).

---

## Verdict

**Implementable as written, modulo the six MAJORs — none of which moves a wire
byte.** The wire layer (§§2–15) is specified to golden-vector grade: the codec,
constants, timers, and state machines are complete enough that two independent
implementations would interoperate. Every MAJOR sits at the same seam — the
§16 core/shell API surface and the state-accounting edges the wire never sees
(closing-state capabilities, pre-establishment stream IDs, the incoming-supply
claim model, wake events, credit true-up for unread bytes, demoted mid-state
supersession) — and each is fixable with a paragraph, not a redesign. The
external critical path is exactly one item: hiss A.1 (split msg1 read) gates
§6 and nothing else; A.2 and A.3 have shipped-today interims (one already in
production code) that the spec should bless explicitly. Build-effort split:
roughly **one-third port** (session/replay window widening, mac1 re-keying,
the RTT/loss/PTO core of recovery.rs, FlakyWire, wire-constant skeletons, the
handshake `noise!` invocation) and **two-thirds green-field** (both sans-io
cores and the poll contract, the shell with waker maps, the entire streams/
reassembly/flow-control/MAX_STREAMS layer, datagram queues, CLOSE lifecycle,
delayed-ACK, NewReno + persistent congestion, the varint frame codec). The
existing actor-shaped endpoint.rs/flow.rs contribute patterns and tests but
not structure.

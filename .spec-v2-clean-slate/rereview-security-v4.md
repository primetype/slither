# Focused adversarial re-review — the post-surgery §§5–7 restructure

**Scope:** `SPEC-v2-DRAFT.md` draft v4 (2026/08/14), the sections the
ratchet-only surgery rewrote — §1.1, §5 (whole), §6 (whole), §7 (whole),
§15.4, §16.1, §16.4, §17.1, §17.4, §18.1, Appendix B's §§5–7 obligations —
read against `rulings.md` (incl. the post-surgery J-1 correction),
`batch-surgery-notes.md` (J-1…J-10 + the six unresolved clauses), and the
frozen pre-surgery text `SPEC-DRAFT-v3-pre-surgery-frozen.md`.

**Verdict: NOT-SOUND** as written. One BLOCKER and five MAJORs; every one
is wire-free to fix (no constant, header, frame type, or packet size moves
under the recommended minimal fixes — two of the MAJORs offer an optional
constant change as an alternative). The restructure's *shape* is right —
one session per connection, staged accept as the only inbound path,
tie-break as the only internal path — but three of its load-bearing safety
claims are false as stated, and the deletion of §7.6 removed a liveness
backstop nothing replaced.

Counts: **1 BLOCKER, 5 MAJOR, 5 MINOR, 2 NOTE.**

---

## SECV4-1 — BLOCKER — proven-LIVE replacement has no replay basis for any connection we established as initiator (or as tie-break winner): a passively captured msg1 destroys live connections

**Clauses.**

§6.4, third bullet: "if a LIVE connection exists for the proven static,
the `accept()` **is** the replacement … and the strictly-greater timestamp
guard is what bars a stale or replayed candidate from ever reaching this
admission (§17.1)."

§17.1: "The guard is also what makes the proven-LIVE replacement of §6.4
safe: a stale or replayed initiation can never carry a strictly-greater
timestamp, so it can never reach a replacing `accept()`."

§17.1 honesty clause: "… never the destruction of a live connection
(pinning protects established statics — **a live static's guard entry is
never evicted, so its replays still die at the guard** — …)".

§5.4, LIVE bullet: "the per-static greatest-timestamp guard (§17.1) blocks
stale or replayed candidates from ever admitting."

Appendix B, local-state routing obligation: "a replayed one additionally
dies at the guard".

**The defect.** All four claims presuppose that a live connection implies a
guard entry for its peer's static whose value is ≥ any msg1 that peer ever
emitted to us. §17.1's own admission list falsifies that: "Admission (check
**and** record) happens at `authenticate()` on the staged path, at a
re-homed `accept()`'s candidate admission (§6.4), and at the internal
tie-break's admit step (§6.6 step 4)". **Nothing records a timestamp when we
establish a connection as the *initiator*** — msg2 carries no payload at all
after the surgery (§5.2), so the dialler never learns any timestamp of the
peer's. Three further paths leave the entry absent or rolled back:

1. **Dialler-established connections** (the common case for any client, and
   half of every mesh pair): `connect()` → msg2 → `Install`. No entry is
   ever written for that static by this exchange. Pinning (§17.1 bullet 1)
   protects an entry that *exists*; it never creates one.
2. **Tie-break winner** (§6.7): "the authenticated inbound is silently
   dropped (mid-state discarded, no msg2, **nothing recorded**)" — the
   winner authenticates the loser's msg1 post-`ss`, knows its timestamp, and
   deliberately discards it. The winner is the connection initiator (§9.1
   role stability), so its connection has no basis either.
3. **no-orphan-on-reject** (§17.1 mitigation (i)): "a static authenticated
   and then rejected without ever being accepted writes no orphan (its
   record drops with the chain; **a pre-existing entry reverts**)" — this
   actively *removes* basis that had been written.
4. **Orphan LRU eviction** while no connection exists, followed by our
   dialling that peer: the entry is gone and the new connection re-creates
   nothing.

**The attack (no keys required).** Attacker passively captures one msg1 that
B once sent to A — e.g. from any simultaneous open A won, from any earlier
dial of B's that A declined or never accepted, or from B's ~18-packet
retransmit train after a failed connect. Later, A dials B and establishes
(A is initiator ⇒ no guard entry for B). The attacker injects the captured
msg1 from any source address:

- mac1 is keyed on A's static — public data (§4.3) — so it verifies;
- `src` ∉ hint set (A holds no in-flight dial to B; §6.5 step 2, §17.4
  "Established connections contribute no hints") ⇒ it parks as an ordinary
  `Intro`;
- `read_identity()` → `Claimed` (B), `authenticate()` → the guard has **no
  entry** for B, so the strictly-greater test passes vacuously ⇒ `Proven`;
- `accept()` ⇒ §6.4's third bullet fires: A's working connection to B is
  destroyed (`ConnectionLost::Replaced`, nothing transmitted — §15.4), and
  A installs a session the attacker cannot complete (it holds neither B's
  static nor B's captured ephemeral private key), which dies at
  `DEAD_TIMEOUT`.

B is not told: §15.4 transmits nothing on a replaced connection, so B
discovers the loss only at its own 15 s liveness. Net cost to the victim
pair: ≥ 15 s of outage per injected packet, plus the application-level
reconnect. The attacker's capture of a retransmit train yields ~18
strictly-increasing msg1s, i.e. ~18 repetitions, each of which also works
against the *next* connection A dials, because each new dial again writes no
basis. There is no rate limit on this (§17.1 records the replayed timestamp
at `authenticate()`, so each captured packet is one-shot, but the supply is
the size of the capture).

The only gate is the application's `accept()` decision — and the application
cannot distinguish this from a genuine reconnect: `Proven` exposes
`peer_static()` and `timestamp()` (§6.2) and both are genuinely B's. The
natural server shape (`while let Some(intro) = ep.accept().await { … }`,
auto-accept by allow-list) is fully exposed. §6.9's replacement row calls
this "application-gated", which is true and insufficient: the application
has no signal to gate on.

**Regression vs the pre-surgery text.** Pre-surgery §6.4 read: "if a LIVE
connection exists for the proven static and the chain is **not** a
restart-replacement `Intro` (§6.6 step 3), `accept()` returns
`AcceptError::AlreadyConnected` — an ordinary accept never destroys a live
connection", and the restart-replacement tag could only be set inside the
hint-gated internal path. An off-path replay arriving at an arbitrary
address therefore could not carry the tag and could not replace. The surgery
deleted the tag and `AcceptError::AlreadyConnected`, promoting *every*
proven-LIVE staged accept to a replacement, while the compensating control
it names (the guard) does not exist for initiator-established connections.
This is a genuine widening, not a pre-existing hole restated.

**Minimal fix (wire-free).** Give the replacement decision a basis that
lives on the connection, not in the endpoint-global orphan tier:

- Every established connection stores `replacement_basis: Option<Timestamp>`
  — `Some(t)` where `t` is the timestamp of the msg1 that established it
  when we were the responder (staged accept, re-homed accept, or tie-break
  loser); `None` when we dialled.
- §6.4's admission is amended: a proven-LIVE `accept()` replaces **only if**
  the live connection's basis is `Some(t)` and the candidate's timestamp is
  strictly greater than `t`; otherwise `accept()` returns
  `AcceptError::Stale`. (The zombie case still resolves: a genuinely
  restarted peer's initiation is refused for at most `DEAD_TIMEOUT`, after
  which the stale connection dies at liveness and the peer's next
  5 s retransmit lands on the NONE row as an ordinary fresh accept — §6.8's
  own story, delayed by ≤ 15 s.)
- Recommended companion (closes case 2 and costs one line): **the tie-break
  winner records the loser's timestamp** at §6.7's winner-side drop. The
  write is post-`ss`, so §17.1's "only key-holders write guard entries"
  invariant is preserved; only the "record on full admission only" phrasing
  needs amending.
- §17.1's honesty clause, §5.4's LIVE bullet, §6.4's bullet and Appendix B's
  obligation are then rewritten to describe what the pair (basis + guard)
  actually guarantees — see SECV4-2, which must be fixed in the same pass.

The rejected alternatives, for the record: re-adding a 12-byte timestamp
payload to msg2 (would give the dialler a basis, costs `RESP_PACKET_LEN`
107 → 119 and re-opens a wire ruling); and seeding the basis from our own
wall clock at establishment (unsound — the guard compares the *peer's* clock
readings, and a cross-host comparison would permanently reject a peer whose
clock trails ours).

---

## SECV4-2 — MAJOR — the guard bars replays that are older-or-equal, never a withheld-newer retransmit: the new §6.4/§17.1 text asserts a property the guard has never had, and regresses §17.1's honesty clause from true to false

**Clauses.** The same four quotes as SECV4-1, plus §5.4's "A withheld or
replayed initiation left unaccepted costs one parked `Intro` and nothing
else".

**The defect (independent of SECV4-1 — it bites even where a basis exists).**
By §5.5 step 2 "**Every retransmit is a completely fresh initiation** — new
ephemeral, new random index, new strictly-greater timestamp". So the
retransmit train of a single genuine connect emits a strictly increasing
timestamp sequence, and any member of it that we have not yet admitted is,
by construction, strictly greater than the entry we did admit. That is
exactly the packet round 1's SEC-5 identified: an on-path attacker drops the
n-th retransmit while the connection establishes on the (n−1)-th, then
injects the withheld packet at a time of its choosing. It passes the guard,
proves B, and (post-surgery) is a fully-qualified replacement candidate.

Round 1 accepted this residual by moving the teardown to `accept()` — a
withheld initiation "left unaccepted costs one parked `Intro` and nothing
else", which is true and is the honest statement. The surgery then added
new text (§6.4 bullet 3, §17.1's new sentence, Appendix B's "a replayed one
additionally dies at the guard") claiming the guard *bars* such candidates,
which is false and, worse, will be implemented as a test that only passes
because it exercises the older-or-equal case. §17.1's pre-surgery honesty
clause said only "pinning protects established statics, and even the
restart-replacement path defers its teardown to `accept()`" — true. The new
parenthetical "so its replays still die at the guard" is not.

**Minimal fix (documentation, wire-free), to land with SECV4-1's fix.**
Replace the three claims with what is actually guaranteed:

> The guard rejects any candidate whose timestamp is ≤ the greatest this
> endpoint has admitted for that static, and (with the §6.4 basis rule) any
> candidate not strictly newer than the initiation that established the live
> connection. It does **not** distinguish a withheld genuine retransmit from
> a fresh one — a strictly-greater initiation captured and injected later is
> indistinguishable from the peer reconnecting. That candidate destroys
> nothing until the application accepts it; applications that auto-accept
> replacements accept that residual.

Appendix B's obligation should be split into the two cases it actually
pins (older-or-equal replay ⇒ dies at the guard; withheld-newer replay ⇒
surfaces as an `Intro` and destroys nothing until accepted).

---

## SECV4-3 — MAJOR — `accept()` consults LIVE but never PENDING: `read_identity()` before `connect()` bypasses the only interception point and yields two concurrent connections to one static

**Clauses.**

§6.5 step 4: "**`read_identity()` interception (the backstop):** when a
parked `Intro`'s claimed static turns out to be a pending outbound remote,
the endpoint performs the same internal tie-break …" — the interception is
specified at `read_identity()` and nowhere else.

§5.4 responder rule: "always consulted **post-`ss`**, at the point each path
proves the static: the tie-break's routing (§6.6) or the staged chain's
admission (§6.4)" — with §6.4's admission testing only "if a LIVE connection
exists for the proven static".

§16.1: "`connect()` to a static with a live `Connection` or an in-flight
outbound connect returns `ConnectError::AlreadyConnected`" — a *staged
chain* in progress is not in that list, and cannot be (at `Claimed` the
static is merely claimed; §6.1 forbids keying anything durable on it).

**The scenario (no attacker needed).** A mesh/peer-to-peer application with
both an accept loop and a dial loop:

1. B's msg1 arrives at A while A holds no pending for B ⇒ parks as an
   ordinary `Intro` (state NONE).
2. A's accept loop calls `read_identity()` ⇒ `Claimed(B)`. No interception
   fires: at this instant B is not a pending outbound remote.
3. A's dial loop calls `connect(addr_B, B)` ⇒ permitted (no live connection,
   no in-flight connect) ⇒ B becomes PENDING.
4. A's accept loop calls `authenticate()` (no interception is specified at
   this verb) ⇒ `Proven`, then `accept()`. §6.4's admission tests LIVE only;
   B is PENDING, not LIVE ⇒ A installs a connection **as responder** and
   sends msg2 — while A's own outbound initiation to B is still in flight.
5. A's msg1 reaches B, B's tie-break (or staged accept) answers it, and A's
   pending completes with `Install` ⇒ **A now holds two connections to
   static B**, from two different sessions, one of which B may have
   discarded.

This violates §16.1 verbatim ("never a second concurrent connection"), and
it falsifies §6.6 step 2's "LIVE and NONE statics are unreachable in this
path" — once step 4 has installed, the static is simultaneously LIVE and
PENDING, which §5.4 asserts "§16.1 forbids". §17.4's `static → connection`
map has one slot and two owners; §6.4's "which connection does this
initiation replace" (§16.1) again has no answer.

**Minimal fix (wire-free).** Give §6.4's admission a PENDING branch,
symmetric with its LIVE branch:

> **PENDING at admission.** If an in-flight outbound initiation exists for
> the proven static, the `accept()` cancels it — the pending and its index
> are dropped and its `Connecting` resolves
> `Err(ConnectError::AlreadyConnected)` — and the accept proceeds as an
> ordinary fresh install. (The tie-break does not run here: the tie-break's
> job is to make two crossing *initiations* converge on one session, and
> this path has already committed to ours as responder.)

Appendix B gains the obligation: `read_identity()` → `connect()` →
`accept()` on the same static yields exactly one connection.

---

## SECV4-4 — MAJOR — a *replay* can cancel an in-flight pending where a forgery cannot: §6.7's guarantee is true but incomplete, and the tie-break path has no basis either

**Clause.** §6.7: "**The tie-break runs only on an authenticated inbound —
after `ss` succeeds.** A match detected at `es` selects the tie-break path
but decides nothing: an `ss` failure is a silent drop with the pending
untouched — **a forgery cannot cancel a pending**." §6.6 step 2 adds:
"**Guard** … strictly greater, or the initiation dies here (a replayed
genuine msg1's shape)."

**The defect.** Step 2's parenthetical assumes the guard holds an entry for
that static. Where it does not (SECV4-1's cases 1–4 — most sharply: we have
never admitted any initiation from this peer, which is precisely the state
of a first-ever mesh dial), a *replayed genuine* msg1 passes `ss`, passes the
guard vacuously, and reaches step 3. If our static is the larger of the pair
(a deterministic 50 % of peer pairs), we lose the tie-break and step 4 then
**cancels our own pending** (§6.7: "we cancel our pending now … the pending
and its index dropped; no give-up, no error"), mints msg2 to the replay's
source, and resolves our `Connecting` **successfully** via
`Install { initial: true }` with a session the attacker cannot complete. The
application is handed a `Connection` that never carries a byte and dies at
`DEAD_TIMEOUT`.

Unlike SECV4-1 this needs **no application decision at all** — the tie-break
is the endpoint's internal path. Each captured msg1 is one-shot (step 4
records the timestamp), so the impact is bounded at ~15 s of denial per
captured packet, and an attacker holding a capture of a full retransmit
train can deny a pair several minutes of connectivity *off-path*, long after
the capture. It is a strictly weaker attack than SECV4-1 and I rate it
MAJOR, not BLOCKER, but it is the same missing-basis root cause and it
falsifies a sentence that is currently set in bold.

**Minimal fix (wire-free).** (a) The companion fix already recommended in
SECV4-1 — **record the timestamp on the tie-break's winner-side drop** —
plus (b) an honest restatement in §6.7:

> A forgery cannot cancel a pending. A **replay** of a genuine msg1 can,
> until the guard holds an entry for that static: replay and first
> transmission are indistinguishable when we have never admitted an
> initiation from that peer. The cost is bounded — the admission records the
> timestamp, so each captured initiation is single-use, and the resulting
> session dies at `DEAD_TIMEOUT` — and it is strictly weaker than the
> capture-capable attacker's baseline ability to drop our handshake.

---

## SECV4-5 — MAJOR — deleting §7.6 removed the absolute session killer, and §7.4's arming rule leaves a connection with un-ACKed data and an idle application immortal

**Clauses.**

§7.4: "The connection is dead when `now − last_authenticated_recv >
DEAD_TIMEOUT` **and** at least one marking send has occurred since that last
authenticated receive." Quiet (non-marking) set: "pure ACKs, PTO probes,
**retransmissions**, the credit frames …".

§7.5: "**Liveness** (`DEAD_TIMEOUT`) … is **the only idle killer**".

§7.6 (deleted): "`REKEY_AGE` (120 s), `REJECT_AGE` (180 s), the `NeedsRekey`
signal …" — and §18.1 lost `ConnectionLost::RekeyFailed` with it.

**The failure.** Take the commonest request/response shape: the application
writes a request that fits inside cwnd and credit, so every byte is
transmitted at least once; the peer ACKs part of it and then vanishes
(crash, blackhole, power loss). From that last authenticated ACK onward:

- every subsequent send is a **retransmission** or a **PTO probe** — both
  explicitly quiet, so **no marking send occurs**, so the death deadline
  never arms and `DEAD_TIMEOUT` never fires;
- the passive keepalive cannot rescue it: §7.5's rule requires "a side that
  has received since it last sent", and we have sent (probes) since our last
  receive, so the keepalive never triggers;
- §13.3's `Pto` stays armed (the sent map is non-empty), backing off to
  `PTO_BACKOFF_CAP` = 2⁶ and probing forever; §14.4 persistent congestion
  collapses cwnd to `MINIMUM_WINDOW` but kills nothing.

The connection is **immortal**: `ConnectionLost` never fires, the
application's `read`/`finish` never resolves, and the endpoint holds the
connection's full state (credit commitment ≤ 1 MiB, replay window, sent map,
stream tables — §17.5) indefinitely. This is reachable by accident and
trivially inducible by an attacker that can drop one direction after
observing a partial ACK.

Pre-surgery this was bounded: §7.6's `REKEY_AGE` fired a re-handshake at
120 s and the failure resolved as `ConnectionLost::RekeyFailed` by
`REJECT_AGE` = 180 s. The ratchet-only ruling deleted the trigger, the
timer, and the variant, and §7.5 now claims `DEAD_TIMEOUT` is "the only idle
killer" — which is exactly the problem: it is the only killer and it does
not fire here. §13.3's J-7 rewrite ("the probe train is ended by liveness
… under asymmetric loss too") is the clause that inherits the false
assumption.

**Minimal fix (wire-free, no new constant).** Amend §7.4's arming condition
from "at least one **marking** send" to "at least one **marking or
ack-eliciting** send" (equivalently: the deadline is armed whenever an
ack-eliciting packet is in the sent map). An ack-eliciting send is by
definition a statement that we expect to hear back, so 15 s of silence after
one is death. This preserves both properties §7.4 was written to protect —
"a sender writing into a black hole dies 15 s after its last authenticated
receive no matter how often it writes" (arming is once, never re-armed) and
"an unending PTO train must not defer death" (probes still do not *defer*;
they now *arm*) — and it makes §13.3's J-7 sentence true. Add the Appendix B
obligation: a fully-transmitted, partially-ACKed request whose peer vanishes
dies at `DEAD_TIMEOUT`, with the probe train ended.

---

## SECV4-6 — MAJOR — the idle keepalive geometry tolerates zero loss, and with the re-handshake deleted a single lost keepalive is now terminal

**Clauses.**

§7.5: "Passive rule: a side that has received since it last sent, and has
not sent for `KEEPALIVE_TIMEOUT`, sends a keepalive." … "It is **the only
idle killer** — an idle session sustained by the keepalive dance keeps
receiving, so **it lives indefinitely**."

Appendix B (new obligation): "an idle keepalive-sustained connection lives
indefinitely with no handshake ever re-run (§5.4, §7.5)."

§5.7: "`DEAD_TIMEOUT` | 15 s | keepalive + one retransmit interval of grace".

**The arithmetic.** In the idle dance each side beats every
`KEEPALIVE_TIMEOUT` = 10 s measured from its own last send, so each side's
inter-arrival gap is 10 s against a `DEAD_TIMEOUT` of 15 s: **5 s of
margin, i.e. tolerance for zero lost keepalives.** Drop one beat and the
receiving side's gap becomes 20 s > 15 s ⇒ `ConnectionLost::TimedOut` — and
the loss is self-reinforcing, because the side whose beat was lost also
fails §7.5's "has received since it last sent" precondition and stops
beating too. Keepalives are never retransmitted (§8.7: PING/ACK/keepalive
are class **never**; §7.5: keepalives "never reach recovery"), so nothing
repairs the gap. On a 1 % loss path an idle connection therefore dies about
every 100 beats ≈ every 17 minutes, and on a 5 % path every ~3 minutes.

WireGuard's 10 s/15 s pair, which §5.7 cites as the derivation, is not a
death timer at all: at Keepalive-Timeout + Rekey-Timeout WireGuard *starts a
new handshake* and the session survives to Reject-After-Time. slither
inherited the geometry and — with §7.6 deleted — dropped the recovery, so
the same 15 s now terminates the connection with no protocol-level repair:
the application must reconnect. §7.5's "lives indefinitely" and Appendix B's
new obligation are true only on a lossless wire, which is exactly the wire
`FlakyWire` is built not to be.

**Minimal fix.** Either is acceptable; both are wire-free:

- **(a) Honest statement only (no constant moves):** §7.5 and the Appendix B
  obligation say "on a lossless path"; add the ruled consequence — "a single
  lost keepalive on an idle connection ends it; there is no re-handshake,
  and reconnection is the application's" — so operators can size
  `PERSISTENT_KEEPALIVE` and reconnect policy against it.
- **(b) Restore one-loss tolerance (one ratified constant moves, needs a
  ruling):** `DEAD_TIMEOUT` 15 s → 25 s (= 2 × `KEEPALIVE_TIMEOUT` + 5 s
  grace), which keeps the persistent-keepalive floor coherent
  (`PERSISTENT_KEEPALIVE` default 25 s ≥ `DEAD_TIMEOUT`) and costs 10 s of
  detection latency; or `KEEPALIVE_TIMEOUT` 10 s → 5 s at the cost of 2×
  the idle beacon rate.

My recommendation is (b) with (a)'s sentence kept as the honest residual for
two consecutive losses — but this is a maintainer ruling, not a reviewer's
call, since it moves a ratified constant.

---

## SECV4-7 — MINOR — §1.1's old-binary note states a suite-specific mechanism as if it were universal (D / J-1 adjudicated)

**Clause.** §1.1: "a development binary speaking the old wire is
distinguished **structurally, not cryptographically**: its mac1 is keyed
over a different static encoding (the 33-byte compressed SEC1 form; this
wire keys over the 65-byte canonical uncompressed form — §2.4, §4.4), so
every cross-wire handshake packet dies at the mac1 gate as a silent drop
before any DH, and no session can ever form between the two wires."

**Verified independently, against the artefacts rather than the log.**
`SPEC.md` line 24 fixes the old wire to a single suite ("Curve | P-256
(secp256r1)"), line 28 keys mac1 on "33-byte compressed SEC1", lines 103–107
give `TIMESTAMP_LEN` 12, `IK_MSG1_LEN` 174, `IK_MSG2_LEN` 81,
`INIT_PACKET_LEN` 196, `RESP_PACKET_LEN` 107, line 146 gives
`PROLOGUE = b"slither\x01"`, line 36 `VERSION 0x01`; and the shipped code
agrees (`src/mac.rs:40,45` — `BLAKE2b-256(MAC1_LABEL ‖
recipient.to_compressed())`, `src/session.rs:14` — `P256r1PublicKey`, no
generic suite anywhere). So:

- J-1's core correction is right and the surgery notes' recomputed sizes are
  right: **the packet sizes no longer separate the wires** (196/107/174/81
  and a 12-byte msg1 payload are identical on both).
- The note's conclusion is **correct and adequately conservative** for every
  binary that actually exists: old binaries are P-256-only, and against them
  mac1 keying differs ⇒ silent pre-DH drop ⇒ no session ⇒ the frame-layer
  divergence J-1 worried about is unreachable.
- The **rulings-log J-1 residual is over-stated**: there is no old X25519
  binary to be byte-compatible with, because the old wire has no X25519
  form. A *new* X25519 endpoint meeting an old P-256 binary separates at the
  §3.1 length gate (114/74 vs 196/107), not at mac1 — which is the one
  respect in which §1.1's sentence over-reaches: it names mac1 as the
  universal gate.
- The hypothetical failure mode, had a handshake completed, **is** safe and
  is correctly described elsewhere: §8.2 parse-then-apply means "Nothing
  from the packet is applied", the death is signalled (CLOSE with
  `PROTOCOL_VIOLATION`, `ConnectionLost::ProtocolViolation { code }`), work
  is bounded (one parse + one CLOSE seal + a 5 s linger capped at ≤ 1
  reply/s to authenticated window-fresh inbound only — §15.2), and no
  amplification arises (the reply is inside the seal to a mutually
  authenticated peer). No state corruption, no unbounded work.

**Minimal fix (one sentence).** Name both gates and scope each:

> For the reference suite (P-256) the separation is mac1's keying encoding
> (33-byte compressed then, 65-byte canonical now — §2.4, §4.4): every
> cross-wire packet fails mac1 before any DH. For any other suite the old
> wire has no counterpart at all — it was P-256-only — so its packets differ
> in length and die at §3.1's gate. Either way no session forms; and were one
> ever to form, the old frame grammar inside the seal fails structurally and
> dies signalled under §8.2, applying nothing.

---

## SECV4-8 — MINOR — unresolved clause 1 (simultaneous open behind port-rewriting NAT) is a real, bounded liveness hole that needs an honesty sentence

**Adjudication: real defect, needs a spec sentence; optional bounded fix.**

The dependency chain is as the surgery notes state, and I confirm the
hint-set shrinkage did not worsen it (both sides are PENDING in a
simultaneous open, and §6.5 step 2's hint set is exactly "the dialled
addresses of all in-flight outbound initiations", so each side's dialled
address remains a hint). Two things are worth recording that the notes do
not say:

- The **self-heal via retransmit does not work** for the NAT case, contrary
  to the natural reading: §5.5's retransmits leave from the same rewritten
  source port, so they miss the hint set exactly as the original did. The
  only backstop is §6.5 step 4's `read_identity()` interception, which
  requires the application to probe a parked `Intro` it has no reason to
  probe. Two pure-dialler applications behind port-rewriting NATs therefore
  fail the simultaneous open on both sides at `HANDSHAKE_GIVEUP` = 90 s.
- The failure is **self-limiting in practice** (both sides then retry at
  application-chosen, hence de-synchronised, times), so I rate it MINOR.

**Minimal fix (documentation).** One sentence in §6.5 after the false-negative
clause:

> The false negative does not self-heal through retransmission — a peer
> dialling from a rewritten source port sends every retransmit from that same
> port — so the `read_identity()` interception is the only backstop and it is
> application-driven: two peers that dial simultaneously, are both NAT-port-
> rewritten, and never probe inbound intros will both fail at
> `HANDSHAKE_GIVEUP` and must retry. Applications that dial SHOULD also drain
> `accept()`.

If the maintainer wants a mechanism rather than a sentence, the bounded form
is: on `connect(S)`, eagerly `es`-read at most `INTRO_MAX_PER_SOURCE` (4) of
the currently parked intros (4 DH, once per dial, attacker-bounded by the
same cap) and route any that claim `S` into the tie-break. That is a new
mechanism and needs a ruling; the sentence does not.

---

## SECV4-9 — MINOR — with pacing deleted (J-2) nothing bounds replacement churn against an auto-accepting application

**Adjudication of J-2: the deletion is right, but it leaves a stated gap
unstated.** Pacing's only home was the internal continuation, which is gone;
re-homing it to `accept()` would have needed a new error variant, correctly
declined as an invention. The residual: a key-holder (a peer whose static
key leaked, or a peer misbehaving) can mint fresh strictly-greater
initiations at line rate, and an auto-accepting application will replace its
own connection on each — 4 DH plus a full connection teardown/rebuild per
accept, plus §17.5's state churn. The blast radius is correctly confined
(§16.1 keys the replacement on the *proven* static, so a key-holder can only
destroy connections to itself — **confirmed: a key-holder harms nobody but
itself**), and §6.9's row is right that the spend is application-chosen.

**Minimal fix (one sentence, §6.4 or §6.9).**

> The protocol paces nothing here: a key-holding peer can present a fresh,
> strictly-greater initiation as fast as it can send, and each accepted one
> costs a teardown plus 4 DH. Applications that auto-accept replacements
> SHOULD rate-limit replacing accepts per static; the protocol's only
> guarantee is that the damage is confined to the connection to that static.

(The alternative the notes offer — a 20 ms gate on replacing accepts — is a
maintainer call; the sentence is enough to keep the spec honest.)

---

## SECV4-10 — MINOR — §7.7's epoch-death justification is wrong as reasoning (the conclusion survives)

**Clause.** §7.7: "a legitimate peer can only get there across ≥ 65 536
messages of silence, which `DEAD_TIMEOUT` excludes by orders of magnitude".

**The defect.** The condition is not silence, it is *unopened* messages: the
receiver's committed epoch advances only on messages that open, so a sender
whose direction is black-holed drifts ahead while the reverse direction
keeps both sides alive. Crossing `MAX_EPOCH_JUMP` = 2 needs 196 608 unopened
seals — reachable inside 15 s at ~13 k pkt/s (≈ 126 Mbps at
`MAX_DATAGRAM`), which is not "orders of magnitude" away for a LAN-rate
sender.

**Why the conclusion nevertheless holds** (and what the text should say
instead): congestion control stops it long before liveness does — with the
forward direction black-holed no ACKs arrive, cwnd collapses to
`MINIMUM_WINDOW` and §14.5's admission gate stops the sender within one RTT,
so the drift cannot accumulate; and if the condition somehow arises, §7.7's
own liveness argument closes it (nothing opens ⇒ `last_authenticated_recv`
freezes ⇒ death at `DEAD_TIMEOUT`, now unconditionally so once SECV4-5's
arming fix lands). "Implementations must not chase epochs" stands.

**Minimal fix.** Replace the clause with "a peer can only drift there across
≥ 196 608 *unopened* messages, which the congestion controller's ACK-driven
admission gate (§14.5) prevents from accumulating and which liveness closes
if it ever does".

---

## SECV4-11 — MINOR — J-3 adjudicated: `Install { initial: true }` is correct; the field should go, not stay

**Clause.** §16.4: "`struct Install { session: EstablishedSession, initial:
bool }   // initial: always true in this revision (no rekey swap — §7.6)" …
"`initial: false` — the deleted rekey swap's value (§7.6) — is emitted on no
path in this revision; the field is retained in shape only."

**Adjudication.** The reviser's reading is right and the walkthrough note's
literal form was wrong: the tie-break loser's `Connecting` is resolved by
`ConnEvent::Established`, which fires on the install, so `initial: false`
there would either strand the `Connecting` or redefine `Established` — a new
mechanism. **Keep `initial: true` for both the msg2 completion and the
tie-break admission.** I verified the completion wiring is otherwise closed:
`accept()` returns an established connection and is "never followed by an
`Install`"; `Install` targets only a `connect()`-created connection,
"exactly once"; "Emitting a symmetry `Install` after `accept()`
(double-install) and waiting for one that never comes are both excluded";
and §16.9 assigns stream IDs at install so both tie-break outcomes produce
identical on-wire parity. **No simultaneous-open path double-installs.**

The residual is the field itself: a `bool` that is always `true` is a trap —
an implementer will eventually branch on it, and a reviewer will eventually
read the branch as live. Delete the field (§16.4's struct, its bullet, and
§6.6 step 4's / §6.7's `Install { initial: true }` mentions become
`Install`); it is the two-line edit J-3 offers, and nothing else references
it.

---

## SECV4-12 — NOTE — the remaining unresolved clauses (2, 3, 4, 5, 6), adjudicated

- **2 — §12.2's fused-ACK upgrade note ("sustained > 100 Mbps per
  connection").** Fine as-is. Nothing in the surgery touches the ACK record;
  §7.2's ruling and the Appendix B burst simulation still gate it.
- **3 — preamble round-1 cluster text (A and F).** Fine as-is. The
  "partially superseded" annotation is the right call: clusters A and F are
  historical record, and rewriting a change log to match a later ruling
  destroys the audit trail. One caveat worth a footnote: cluster F's "idle
  sessions rekeying via the keepalive" is now *entirely*, not partially,
  superseded — the annotation covers it, but a reader diffing F against §7.5
  will find nothing left of that half.
- **4 — "the v1 shell" / "the one v1 implementation" (§14.1, §14.7).** Fine
  as-is; under ruling 1 they now read consistently ("v1" = this wire and its
  first shell). No edit needed.
- **5 — `ConnEvent::Established`'s "first install" comment.** Fine as-is
  under SECV4-11's recommendation; if the `initial` field is deleted, drop
  "first" in the same pass ("the install") since exactly one ever occurs.
- **6 — §6.2's staged-verb signatures.** Fine as-is; `authenticate()` never
  exposed flags, so nothing needed to change. One observation rather than a
  defect: post-surgery, `accept()` can destroy a live connection and the
  staged API surfaces nothing about that — `Proven` exposes `peer_static()`
  and `timestamp()` only. The application *can* discover it (it holds the
  `Connection` and can key its own table on the static), and SECV4-1's fix
  makes the decision safe, so no API change is required; a sentence in §6.2
  pointing out that accepting a `Proven` whose static already has a live
  connection replaces it would save an implementer a surprise.

---

## SECV4-13 — NOTE — what survived the attack (positive confirmations)

- **Anti-amplification arithmetic (F) — verified sound.** §2.3's algebra
  recomputes correctly at the new sizes: `IK_MSG1_LEN` = 65 + (65+16) +
  (12+16) = 174, `INIT_PACKET_LEN` = 6 + 174 + 16 = **196**; `IK_MSG2_LEN` =
  65 + 16 = 81, `RESP_PACKET_LEN` = 10 + 81 + 16 = **107**. §6.9's "No
  amplification" holds: 107/196 = 0.546 < 1. The anchoring initiation's
  funding claim holds with room: §7.3 arms the budget at 3 × authenticated
  bytes received = 3 × 196 = **588 B** against a 107 B msg2, leaving 481 B of
  headroom for the responder's first output, and §7.3's authenticated-bytes
  rule correctly qualifies the anchoring msg1 ("its handshake tail tags
  having verified at admission"). The surgery's −1 B on each size moved the
  headroom from 483 B to 481 B; nothing depends on the difference. The
  initiator anchors at a *dialled* address, so no budget arms there —
  correctly, since it chose the address itself.
- **§7.9's exhaustion arithmetic — verified.** 2⁶⁴ − 2 = 1.845 × 10¹⁹; at
  10⁷ pkt/s = 1.845 × 10¹² s = 58 450 years ("over 58 000" ✓). Varint cap
  2⁶² − 1 = 4.612 × 10¹⁸ = 14 614 years ("over 14 000" ✓, and §8.1's twin
  statement agrees). Per-epoch-key volume 65 536 × 1 186 B = 77 725 696 B ≈
  78 MB ✓ (and 1 186 = `MAX_PLAINTEXT` 1 170 + 16 ✓), comfortably inside any
  ChaCha20-Poly1305 per-key bound.
- **No orphaned rekey machinery.** A full-document sweep for `REKEY_AGE`,
  `REJECT_AGE`, `NeedsRekey`, "swap", "make-before-break", "survives rekey",
  `CONTINUATION`/`CONTINUED`, `PeerRestarted`, `RekeyFailed`,
  `AcceptError::AlreadyConnected`, `Superseded`, `AddressMoved` (as
  `ToEndpoint`), `INITIATIONS_PER_SECOND` finds them **only** in the §7.6
  deletion stub, the §7.5 moot-half note, the preamble's historical cluster
  block, and the §18.1/§6.3 "appears nowhere" statements. §2.1's hiss-contract
  row, §7.1, §7.8, §9.1's role stability, §9.2's watermark, §10.7, §11.5,
  §13.1/§13.6, §14.6, §15.4, §16.5, §16.10, §17.4 and §19 were all checked
  for surviving "a session can be replaced without replacing the connection"
  assumptions: **none survive.** The one-session-per-connection collapse is
  clean.
- **Key-holder blast radius.** Confirmed confined: every replacement decision
  keys on the *proven* static (§6.4, §16.1), so a key-holder can destroy only
  the connection to its own static. It cannot touch a third peer's
  connection, cannot write another static's guard entry (§17.1's
  post-`ss`-only write path), and cannot cancel a pending it does not own
  (§6.7's tag step).
- **Forgery cannot cancel a pending** (as distinct from replay — SECV4-4):
  confirmed. A spoofed hint-set source claiming a pending-outbound static
  costs 2 DH and dies at the msg1 tail tag with the pending untouched.
- **Tie-break determinism.** Confirmed closed: equal-length canonical
  octets, no `Ord` bound, evaluated identically on both sides, equal statics
  unrepresentable under §16.1, exactly one session (winner's msg1 + loser's
  msg2) constructed and held by both, stream parity fixed at establishment
  (§6.7, §9.1, §16.9). The asymmetric arrival order (msg1 before the local
  `connect()`) resolves either at the `read_identity()` interception or at
  the next 5 s retransmit through the eager path — except in SECV4-8's NAT
  case and SECV4-3's ordering.
- **Idle keepalive dance geometry** (loss aside — SECV4-6): the 10 s beat
  against a 15 s deadline, with the keepalive classified marking and PTO
  probes classified quiet, is internally consistent, and the
  `PERSISTENT_KEEPALIVE ≥ DEAD_TIMEOUT` floor is coherent with it.
- **An improvement the surgery bought, worth recording:** msg2 loss now
  repairs faster. Pre-surgery, the initiator's next retransmit hit
  `AcceptError::AlreadyConnected` at the half-open responder and recovery
  waited on liveness; post-surgery the application can accept the
  replacement immediately. Appendix B should pin it (msg2 lost ⇒ the
  initiator's retransmit surfaces as a replacement `Intro` ⇒ accepting it
  restores service without waiting 15 s).

---

## Verdict

**NOT-SOUND** for the restructured sections as written — `§5.4`, `§6.4`,
`§6.5–6.7`, `§7.4–7.5`, `§17.1` and the Appendix B obligations that mirror
them need the fixes above before ratification. The restructure's
architecture is sound and, in three respects (one session per connection,
staged-accept-only inbound, teardown at the committing act), strictly better
than the machinery it replaced; what fails is the *compensating control*.
Deleting the continuation flags removed the tag that used to keep an
arbitrary inbound initiation from replacing a live connection, and the guard
that was promoted to take its place does not, and never did, cover the
initiator-established case (SECV4-1/2/4). Separately, deleting §7.6 removed
the last absolute session killer without noticing that `DEAD_TIMEOUT` does
not fire in one common configuration (SECV4-5), and left the keepalive
geometry with no recovery mechanism behind it (SECV4-6).

All six blocking/major fixes are wire-free: no packet size, header layout,
frame type, error code, or ratified constant needs to move under the
recommended minimal forms (SECV4-6 offers an optional constant change as the
stronger alternative, which is a maintainer ruling). Re-review of §6.4,
§6.7, §7.4 and §17.1 after the fixes land is warranted, since SECV4-1's and
SECV4-3's fixes both add branches to `accept()`'s admission.

*Reviewed 2026/08/14 against `SPEC-v2-DRAFT.md` draft v4,
`SPEC-DRAFT-v3-pre-surgery-frozen.md`, `SPEC.md`, and `src/` (mac1 keying
and suite genericity verified in code).*

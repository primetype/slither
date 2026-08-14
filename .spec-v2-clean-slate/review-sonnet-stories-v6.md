# Fidelity review — STORIES.md vs SPEC-v2-DRAFT.md (sonnet, v6)

**Method.** `STORIES.md` read in full. Each story's cited anchors checked
against `SPEC-v2-DRAFT.md` text via targeted `grep -n` + `Read` with small
offsets — the whole spec was never loaded at once. `.spec-v2-clean-slate/rulings.md`
consulted in full for rulings 6, 10, 11, 30–45 (the numbers the stories or
their walkthrough cite). Where a story's claim implied a type also exists
in the actual crate (`testutil`), `src/` was checked as a secondary,
non-authoritative cross-check — the spec text is what governs fidelity.

---

## Verdict: **CONFORMANT**, with six findings requiring fixes before this
document is used as implementation-plan input.

Every load-bearing mechanical claim checked with "extra care" per the
review brief came back **correct**: the 0/1/2/4 DH ladder (S6/S7/S9),
S3's three-way split against §5.4/§6.4/§17.4, S5's install-pin and
passive-dance bootstrap against §7.4/§7.5, S11's contested-probe mechanics
and ruling 45's `ConnEvent::Contested` against §7.5/§16.4/§16.5, and S23's
ratchet against §7.7 all match the spec precisely, in most cases nearly
verbatim. The front-matter constants block (`KEEPALIVE_TIMEOUT`,
`DEAD_TIMEOUT`, `HANDSHAKE_GIVEUP`, `INTRO_TTL`, `INTRO_QUEUE_CAP`,
`INTRO_MAX_PER_SOURCE`, `PERSISTENT_KEEPALIVE`, `MAX_DATAGRAM`,
`MAX_PLAINTEXT`) is entirely correct.

The six findings below are real but narrow: one wrong enum-variant
spelling in the maintainer's #1 story, one wrong section citation, one
substantive tension between a story's accepted capability and a ratified
normative sentence, one wrong constant name, two unattested (but
crate-real) test-util type names, and one imprecise trigger condition in
the teardown story. None is wire-affecting. None undermines the
substance of the story it appears in — each is a fixable naming,
citation, or scoping defect, not a misunderstanding of the protocol.

---

## Findings

### FID-1 (MAJOR) — S1 names a `ConnectionLost` variant that does not exist

**Claim (STORIES.md:43):** "The peer surfaces `ConnectionLost::Closed`
with the same code and reason (truncated at `CLOSE_REASON_MAX`)."

**Spec reality:** the variant is **`PeerClosed`**, not `Closed`. The
authoritative enum listing, SPEC-v2-DRAFT.md:4318-4319:

> ``ConnectionLost::{TimedOut, NonceExhausted, LocallyClosed, PeerClosed
> { code, reason }, ProtocolViolation { code }, Replaced,
> EndpointDropped}``

and again in the teardown matrix, SPEC-v2-DRAFT.md:3570:

> `peer's CLOSE received | nothing (drain only) | ConnectionLost::PeerClosed
> { code, reason } | (it closed)`

There is no `Closed` variant anywhere in the spec. **Story should
change**: `ConnectionLost::Closed` → `ConnectionLost::PeerClosed { code,
reason }`. Not wire-affecting (pure Rust-API naming). Severity is MAJOR
rather than BLOCKER because the fix is a one-identifier swap and would be
caught immediately at compile time — but it sits in the maintainer's
explicitly-designated #1 story, the first thing an implementer would
build and test against, so it should not ship uncorrected.

### FID-2 (MAJOR) — S2 cites the wrong section for the handshake retransmit schedule

**Claim (STORIES.md:52):** "Retransmissions during the window follow
§13's schedule." Anchor line: "**Anchor:** §16.1, §13."

**Spec reality:** §13 ("Loss recovery") is explicitly scoped to the
**post-handshake data path**, over the established session's
ack-eliciting sent-packet map: SPEC-v2-DRAFT.md:3211-3212, "Per
connection (a connection has exactly one session — §7.8), over the
ack-eliciting sent-packet map. RFC 9002's shape throughout." Its PTO
formula uses RTT estimation and **exponential backoff**
(`2^pto_count`, capped at `PTO_BACKOFF_CAP`, SPEC-v2-DRAFT.md:3262-3263).

Handshake retransmission is governed by **§5.5**, a completely different,
simpler mechanism: SPEC-v2-DRAFT.md:790-793, "Arm a retransmit at
`RETRANSMIT_BASE` + uniform jitter ≤ `RETRANSMIT_JITTER_MAX` (5 s +
U[0, 333 ms])... **Every retransmit is a completely fresh initiation**...
The interval is **fixed, not exponential** — WireGuard's shipped shape,
kept for simplicity." The spec itself draws this contrast explicitly
("fixed, not exponential"), which is precisely what would be violated by
reading the handshake retry as governed by §13's exponential-backoff PTO.

There is no session, no ACK frame, and no sent-packet map before the
handshake completes, so §13 cannot apply to it. **Story should change**:
cite §5.5 (and, for the 90 s ceiling, §5.5 point 6 / §5.7) instead of
§13. Not wire-affecting.

### FID-3 (MAJOR) — S7's denylist capability is unattested, and plausibly contradicts a ratified §6.1 sentence

**Claim (STORIES.md:142-144):** "**Accepts (denylist):** having seen the
static, the application refuses it. Subsequent initiations claiming it
can be dropped at the `Intro` stage for **0 DH** — the denylist is
checked before `read_identity()`."

**Spec reality:** none of S7's cited anchors (§6.1, §6.2, §6.5, Appendix
A.1) describes a denylist mechanism — the string "denylist" (or
"blocklist") does not occur anywhere in `SPEC-v2-DRAFT.md`. More
seriously, §6.1 contains a **ratified** ([RATIFIED 2026/08/14],
SPEC-v2-DRAFT.md:948) sentence that reads as a direct prohibition on
exactly the mechanism S7 describes, SPEC-v2-DRAFT.md:972-975:

> "The claimed static at `Claimed` is attacker-choosable (reaching it
> requires no secret)... **Nothing durable may be keyed on the claimed
> static, the source address, or `sender_index`** — no map insertion, no
> rate-limit bucket, no unbounded logging."

A denylist keyed on the claimed static *is* a durable map insertion keyed
on the claimed static. The passage does not scope itself to "slither's
internal bookkeeping only" — its rationale ("attacker-choosable...
requires no secret") applies identically whether the map lives inside
the endpoint or in the application's own code, and is the same rationale
S7's own `⚠ CHECK` gives for the hazard ("an attacker can claim any
public key to get it banned"). There is also a structural problem with
the mechanism as described: the claimed static is not visible until 1 DH
is paid (`read_identity()`, §6.1's table); "the denylist is checked
before `read_identity()`" is achievable only if the denylist is actually
keyed on **source address**, not on the static the ⚠ CHECK is talking
about — the story conflates the two.

**Recommendation:** either (a) the spec gains an explicit carve-out in
§6.1 permitting *application-level* policy to key on the claimed static
at the application's own risk (distinct from slither's internal state,
which the sentence would continue to bind), or (b) S7's "Accepts
(denylist)" bullet is narrowed to source-address-based denylisting only,
which is fully supported at 0 DH with no tension (`Intro::source()` is
visible pre-`read_identity()`). As written, the story and the ratified
sentence disagree. Not wire-affecting.

### FID-4 (MINOR/MAJOR) — S16 names a constant, `MAX_MESSAGE`, that does not exist

**Claim (STORIES.md:243):** "a message up to `MAX_MESSAGE` in one call;
larger is `MessageError::TooLarge`."

**Spec reality:** the constant is **`MESSAGE_RECV_MAX`** = 262,144 B
(= `INITIAL_MAX_STREAM_DATA`), SPEC-v2-DRAFT.md:2852, and it is enforced
at the send side too (`send_message` rejects payloads above it,
SPEC-v2-DRAFT.md:2837-2839, and the wire-level bound table restates this
at SPEC-v2-DRAFT.md:624). `MAX_MESSAGE` does not occur anywhere in
`SPEC-v2-DRAFT.md`. It also is not among the constants STORIES.md's own
front-matter list declares (that list — line 26-29 — omits it entirely),
so this is a citation the story invents rather than one the front matter
sanctions and the body merely uses loosely. **Story should change**:
`MAX_MESSAGE` → `MESSAGE_RECV_MAX`. This was one of the constants the
review brief specifically asked to be checked by exact spelling, so
flagged at MAJOR despite the trivial fix.

### FID-5 (MINOR) — S24 cites two `testutil` type names the spec never attests (though they are real in the crate)

**Claim (STORIES.md:342):** "**Anchor:** §16.10,
`testutil::{Network, FlakyWire, FlakyPolicy}`."

**Spec reality:** `SPEC-v2-DRAFT.md` names only **`testutil::FlakyWire`**
— at §16.10 (SPEC-v2-DRAFT.md:4029), at §16.3 (line 3749), and in
Appendix B (line 4505). `Network` and `FlakyPolicy` do not appear
anywhere in the spec text. Cross-checked against the actual crate
(`src/testutil.rs`): both types genuinely exist there (`pub struct
FlakyPolicy`, and a `Network` struct at "a shared in-memory network that
routes datagrams between `FlakyWire`s"), and `src/flow.rs`'s test suite
uses all three names together. So this is not a fabrication — it is a
**spec-completeness gap**: the spec's own §16.10/Appendix B under-name
the test-util surface relative to what a paused-clock two-endpoint test
actually needs (something has to construct the shared network and
per-endpoint loss/reorder/duplication policy). Recommend the spec name
`Network` and `FlakyPolicy` alongside `FlakyWire` at §16.10 or Appendix
B, since S24 is right that this is the mechanism "all of the above is
tested" with, and an implementer reading only the spec would not learn
the setup API's shape.

### FID-6 (MAJOR) — S26 states dropping *the endpoint* tears everything down; the spec's actual trigger is dropping *every* handle

**Claim (STORIES.md:355-358):** "S26 — dropping the endpoint tears
everything down cleanly. **Accepts:** every handle observes the
endpoint's disappearance; nothing leaks; no task outlives the
`LocalSet`."

**Spec reality:** §16.3 states the driver-lifetime invariant precisely,
and it is *any handle*, not specifically the `Endpoint`:
SPEC-v2-DRAFT.md:3744-3746, "**The driver lives while any handle lives**;
dropping every handle stops it, and every session dies silently with
it." This is restated at §16.2, SPEC-v2-DRAFT.md:3738: "Dropping every
handle stops the driver and every connection dies silently — nothing
transmitted (§15.4)." And the teardown matrix's row is keyed on the same
condition, not on the `Endpoint` specifically, SPEC-v2-DRAFT.md:3573:
"endpoint dropped — **every handle gone** (§16.3) | nothing | — (the
driver stops) | liveness, ≤ 25 s" — note the *local surface* column is
literally "—" (nothing to observe), because by the row's own precondition
every handle, including whatever would have observed the event, is
already gone.

Read literally, S26's title and accept-criterion describe dropping the
`Endpoint` object specifically while implying live `Connection` handles
would then observe `ConnectionLost::EndpointDropped` and tear down. But
per §16.3 a live `Connection` handle is itself a handle keeping the
driver alive — dropping only the `Endpoint` (with `Connection`s still
held) does **not** stop the driver and does **not** produce
`EndpointDropped` on those connections; they keep running normally.
`EndpointDropped` is instead "the answer **a surviving verb call**
receives when the driver has stopped mid-flight" (SPEC-v2-DRAFT.md:3575-3576)
— i.e., it fires only after the *whole* handle set (endpoint and every
connection) is already gone, at which point, per the teardown-matrix row
itself, there is by construction no handle left to receive that answer
through a normal poll. There is also a second, separate per-connection
mechanism the story does not mention: dropping the **last handle to one
`Connection`** (endpoint and other connections untouched) performs a
*graceful*, wire-signalled `close(NO_ERROR, "")` (SPEC-v2-DRAFT.md:3727-3729)
— the opposite of "nothing leaks... silently," since it does transmit.

**Recommendation:** reword S26 to "dropping every handle" (matching
§16.3's actual precondition) rather than "dropping the endpoint," or add
an explicit accept criterion distinguishing (a) dropping only the
`Endpoint` while connections are held (connections unaffected, no new
`accept()`/`connect()` possible — this case is not specified anywhere
in the spec and may itself be a spec gap worth naming), (b) dropping the
last handle to one `Connection` (graceful CLOSE, wire-signalled), and
(c) dropping every handle at once (silent, nothing transmitted, matches
the current story's "nothing leaks" framing). Not wire-affecting.

---

## Verified correct (checked against spec text, not merely unchallenged)

- **The 0/1/2/4 DH ladder** (S6, S7, S9) — SPEC-v2-DRAFT.md:951-955's
  table and Appendix A.1's pinned-test description (lines 4441-4447:
  "`dhs == 1` after intro... `dhs == 2` cumulatively after `complete()`...
  a third test pins the drop-to-reject path at exactly 1 DH") match S6's
  "0 DH", S7's "1 DH, `ss` never paid", and S9's "2 DH cumulative, 4 DH
  total" exactly, including that rejection at every stage is a silent,
  DH-frozen drop.
- **S1's DH-cost breakdown** ("4 DH: es, ss, ee, se") — confirmed against
  §5.2 (msg1 carries `e, es, s, ss`, line 706) and §5.6 (msg2 processing
  is `ee, se`, line 711/824).
- **S3's three-way split** (S3a/S3b/S3c) — confirmed in full against
  §5.4 (SPEC-v2-DRAFT.md:730-786), §6.4 (1144-1292), §16.1 (3619-3634),
  and §17.4's `replacement_basis` definition (4193-4200: `Some(t)` only
  where "we were the **responder**"; `None` when "we **dialled**").
  Every clause — `AlreadyConnected` for a second local dial with the
  first untouched, replacement gated on `Some(t)` + strictly-newer
  timestamp, `Stale` + contested for a dialled connection — matches the
  spec precisely, including ruling 35's fix to the PENDING branch
  (§6.4's PENDING-branch text and §6.6 step 2 explicitly state the two
  routes "can never disagree").
- **S4's simultaneous-open tie-break** — §6.7 (1398-1470) and §6.6
  (1349-1392) confirm the lexicographically-smaller-static-wins rule,
  the stream-parity consequence, and (via ruling 35's now-applied fix)
  that the staged-route and internal-tie-break route reach the same
  conclusion.
- **S5's reap and persistent-keepalive stories** — §7.4's install-pin
  paragraph (SPEC-v2-DRAFT.md:1812-1829) confirms the idle-from-install
  25 s death with nothing transmitted; §7.5's passive-rule and ruling-39
  text (1979-2050) confirms the one-exchange self-sustaining dance; and
  ruling 44 plus Appendix B (3707-3712, 4696-4699) confirm
  `Result<(), ConfigError>` with `ConfigError::{KeepaliveTooShort,
  KeepaliveTooLong}`, range `[1 s, DEAD_TIMEOUT)`, and "leaves the
  interval unchanged" on rejection.
- **S11's contested-probe mechanics and ruling 45** — §7.5's
  contested-connection-probe subsection (2107-2159) and rulings 36/41/43
  (rulings.md:340-364, 501-551) confirm the probe-floor mechanism, the
  `KEEPALIVE_TIMEOUT` deadline armed at transmission, the one-mark-per-
  connection collapse rule (no re-arm on a second refusal), and
  `ConnEvent::Contested { under_probe: bool }` at §16.4
  (SPEC-v2-DRAFT.md:3838, 3856-3868) is verbatim-consistent with ruling
  45's text, correctly cross-referenced in §16.5's timer table (3913) and
  §15.4's teardown-matrix contested row (3567). It does not contradict
  §16.4's "signals, not payload carriers" framing — that exact phrase is
  reapplied to `Contested` explicitly (line 3866).
- **S18's mover-keepalives obligation** — confirmed against §7.3's
  roaming rule (authenticated-receipt-driven, SPEC-v2-DRAFT.md:1729-1731)
  and §7.5's passive-dance mechanics; the "must send, keepalive is what
  does it" framing matches the spec's own liveness model with no
  transport-side probe added.
- **S23's ratchet** — §7.7 (2296-2331) confirms `REKEY_EPOCH_MSGS` =
  65,536 per direction, no round trip, no handshake, and (checked by
  grep across every "epoch" occurrence in the spec) **no `ConnEvent`** is
  ever emitted for a ratchet transition — S23's "no application-visible
  event" claim holds exactly as stated. "No DH re-handshake anywhere" is
  confirmed by §5.4's ratchet-only ruling text.
- **The front-matter constants block** (STORIES.md:26-29) — every value
  (`KEEPALIVE_TIMEOUT` 10 s, `DEAD_TIMEOUT` 25 s, `HANDSHAKE_GIVEUP` 90 s,
  `INTRO_TTL` 15 s, `INTRO_QUEUE_CAP` 1024, `INTRO_MAX_PER_SOURCE` 4,
  `PERSISTENT_KEEPALIVE` default 10 s / range `[1 s, DEAD_TIMEOUT)`,
  `MAX_DATAGRAM` 1200, `MAX_PLAINTEXT` 1170) matches the spec's own
  tables exactly (§5.7, §6.3, §7.5, §3.5).
- **Other named error variants** — `AcceptError::Stale` (§6.4 throughout),
  `ConnectError::{AlreadyConnected, TimedOut}` (§18.1 line 4283, §5.5
  line 817, §16.1 line 3621), `ReadError::Reset(code)` (§9.6 line 2761,
  §18.1 line 4337), `DatagramError::TooLarge` (§11.4 line 3102, §18.1
  line 4341), `ConnEvent::AddressMoved` (§16.4 line 3833, §7.3 line
  1736) — all spelled correctly and used with matching meaning in their
  stories.
- **S20's restart-handling summary** — matches §6.8
  (SPEC-v2-DRAFT.md:1497-1546) exactly, including the `DEAD_TIMEOUT`
  bound's dependence on "nothing authentic still reaching the zombie"
  and the contested-probe rescue for the attacked case.
- **S8's suspend-across-turns claims** — matches Appendix A.1's mid-state
  description (owned, no lifetime, `#[must_use]`, not `Clone`, "cannot be
  forked") and its explicit rejection of the `_with` synchronous fallback
  (SPEC-v2-DRAFT.md:4459-4463), essentially verbatim.
- **S9, S10, S13, S14, S15, S22** — checked individually against their
  cited anchors (§6.1/§6.2; §6.3/§6.9/§17.5; §9.1; §9.6/§18.1; §11.3-4/
  §18.1; §1.1/§2/§3.1 respectively) with no discrepancies found.

---

## Uncovered spec surface

Normative behaviour in `SPEC-v2-DRAFT.md` with **no story** describing
it — an implementation plan built only from STORIES.md would not build
these:

1. **§18.2's trace-target operator contract** — `slither::policy`,
   `slither::replay`, `slither::frames`, `slither::roam` are declared a
   stable, protocol-revision-gated surface ("renaming or dropping one is
   a protocol revision," SPEC-v2-DRAFT.md:4357-4358), yet no story
   mentions observability/tracing at all. This is a real operator-facing
   capability (distinguishing the two `TimedOut` causes, per
   SPEC-v2-DRAFT.md:3589-3590, depends on it) with zero coverage.
2. **`ConnectionLost::ProtocolViolation { code }`** — the CLOSE-on-
   violation path (§15.2's third bullet, §8.2's semantic/structural
   violation classes) has no story. No accept criterion anywhere asserts
   that a peer sending a credit-breach or stream-state violation gets
   CLOSE'd with the matching code and the local side observes
   `ProtocolViolation`.
3. **`ConnectionLost::NonceExhausted`** (§7.9) — no story exercises or
   even mentions nonce exhaustion. Low priority given its ≈58,000-year
   unreachability at realistic send rates, but it is a distinct,
   documented terminal state with zero test-shaped coverage in the
   story set.
4. **§4 mac1 construction/verification** as a mechanism in its own
   right (keying, verification order, "what mac1 is not") — touched only
   indirectly through S6's "an inbound initiation whose mac1 verifies."
   The DoS-gate property itself (§4.3's explicit "mac1 is not
   authentication of identity") is never asserted as a story.
5. **Congestion control** (§14: NewReno, the recovery period, persistent
   congestion, the roam reset seam) — entirely internal to S12's "survives
   loss" framing; no story exercises congestion-window behavior,
   persistent-congestion detection, or the post-roam cwnd reset
   specifically (S18/S19 cover the roam *address* change, not the
   accompanying congestion-controller reset of §14.6).
6. **The CLOSE error-code registry's application range** (§15.3, codes
   ≥ `0x10`) — S1 exercises `close(code, reason)` generically but no
   story asserts that application-defined codes round-trip, or that the
   registry's reserved range (`0x06`-`0x0f`) is never sent.
7. **§17.5 state ceilings** beyond the intro queue (S10 covers the
   queue; the per-connection and endpoint-wide memory ceiling table
   itself, including the cwnd-plus-exemptions accounting, has no story).

---

## Notes not raised to findings

- **S8** infers `#[must_use]`/`!Clone` for slither's own `Claimed` type
  from Appendix A.1's description of the underlying **hiss** mid-state
  type (`IKResponderMsg1Intro<CP>`) it wraps — the spec never states
  these attributes for slither's own `Intro`/`Claimed`/`Proven` names
  directly. Given the design intent stated at A.1 ("this is what lets the
  `Claimed` stage suspend"), the inference is reasonable and almost
  certainly what's intended, so this is noted rather than raised as a
  finding.
- **S17**'s "a reader that stops reading stalls its own stream, not the
  connection" holds precisely for a *single* stalled stream (its 256 KiB
  per-stream cap binds well before the 1 MiB connection-level cap could).
  The spec does not separately address the aggregate case (many streams
  simultaneously unread, exhausting connection-level credit and thereby
  stalling actively-read streams too) — S17 doesn't claim to cover that
  case either, so no mismatch, just an unexercised corner.
</content>

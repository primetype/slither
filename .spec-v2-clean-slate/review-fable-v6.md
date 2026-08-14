# Fresh-eyes consumer review — SPEC-v2-DRAFT.md + STORIES.md

*Reviewer: Fable (v6 pass), first outside reader. Lens: would a competent
engineer building on this crate succeed, or fall into a hole? Internal
consistency was explicitly out of scope (another reviewer holds it).*

*Method: STORIES.md read in full; targeted reads of §§5.3, 6.4, 7.3–7.5,
9.8, 15, 16, 17.1, 18, 19, Appendix B, the constants table, and ruling 45.*

---

## Verdict

**Ship it — after one wire-free ruling batch, before implementation
starts.** The protocol itself (handshake, staged accept, liveness,
contested probe, recovery, flow control) is unusually rigorous: the hard
security arguments are written down, the honesty clauses admit their own
residual weaknesses, and the constants all trace to derivations. Not one
finding below requires a wire change. But the defects that do exist
cluster in one place: the **core→shell seam** — the API surface the
consumer actually touches (§16.2) — which is visibly the youngest, least
adversarially-reviewed layer of the document. Five findings there are
MAJOR: the "application-visible" events of ruling 45 and S18 have no
application-facing delivery mechanism; close discards unacknowledged data
and no API can wait for delivery; dropping a `Connecting` is unspecified
and interacts badly with `AlreadyConnected`; S25 has no landing zone in
the closed error taxonomy; and the message/uni-stream shared supply is a
documented mechanism whose consumer-facing consequence is documented
nowhere. All five are cheap to rule now and expensive to discover as
bubble-engine bug reports later.

---

## Findings

### FAB-1 — MAJOR — "application-visible" events have no application-facing delivery mechanism

**Expectation:** Ruling 45 makes the contested mark "application-visible"
via `ConnEvent::Contested`; S18's acceptance says "The application
observes `ConnEvent::AddressMoved`"; S11 says a `ConnEvent` reports
contested "and its resolution is observable." A reader expects
`Connection::events()` or equivalent.

**Actuality:** `ConnEvent` is a **core-level** output (§16.4, line 3827).
The shell surface (§16.2, lines 3646–3686) exposes verbs and exactly four
accessors (`remote_static`, `remote_address`, `session_id`,
`is_established`) — no event stream, no callback, no `poll_event`, no
`is_contested()`. §7.3 (line 1734) states the pattern outright for
roaming: *"Observability is the `remote_address()` accessor plus the
`slither::roam` trace target"* — i.e. the core event is shell-internal
plumbing. For `Contested` there is not even an accessor. And connection
**death** has the same shape: every lifecycle notification
(`Closed(ConnectionLost)`) reaches the application only as the error arm
of a parked or future verb call (lines 3655–3662) — an application
holding a connection it is not currently reading learns of its death
never, or by polling `is_established()`.

This is precisely the consumer ruling 45 names: bubble-engine's re-dial
scheduler needs *push* notification of "contested → cleared" vs
"contested → dead" and of "connection down." As specified, an
implementer must invent the delivery mechanism, and two implementers
will invent different ones (broadcast channel vs. watch cell vs. parked
dummy verb).

**Location:** §16.2 lines 3646–3686 (the closed accessor list); §16.4
lines 3856–3875; §7.3 line 1734; ruling 45 (rulings.md lines 589–615);
STORIES.md S11 lines 195–203, S18 line 267.

**Fix (wire-free):** add an event surface to §16.2 — e.g.
`Connection::events() -> EventStream` carrying the app-relevant subset
(`AddressMoved`, `Contested`, `Closed`), or at minimum a
`closed().await`-style future plus an `is_contested()` accessor. Needs a
ruling because §16.2's types are "normative in shape."

**Confidence:** certain.

### FAB-2 — MAJOR — close discards unacknowledged data, and no API can wait for delivery: the "farewell message" is silently lossy

**Expectation:** `send_message(...).await` succeeded, so the message is
sent; `close()` afterwards is safe. This is the natural last act of a
messaging app ("send goodbye, hang up") and of every clean shutdown.

**Actuality:** three spec facts compose into a trap:

1. `send_message` resolves when the payload is written into send state,
   not when acknowledged (§16.2 line 3691, §9.8 lines 2834–2838);
   `finish()` likewise — *"resolves when the FIN is accepted into the
   stream's send state"* (lines 3692–3694).
2. `close()` enters closing where *"all stream, flow-control, recovery,
   and congestion state may drop immediately"* (§15.2 lines 3511–3513) —
   in-flight data is never retransmitted after close, and data still
   queued behind the congestion window is never sent at all.
3. The one primitive that means "fully delivered" —
   `StreamFinished { id }` = *"send half fully acknowledged"* (line
   3833) — is a core-level event with **no shell counterpart** (FAB-1),
   so an application *cannot* wait for delivery even if it knows it must.

So on any lossy path, message-then-close (or message-then-drop, since
last-handle drop performs `close(NO_ERROR, "")`, lines 3728–3729) loses
the tail with probability ≈ the loss rate. QUIC apps share the hazard
but get `stopped()`/`wait_idle`-style outs; slither offers none.

**Location:** §15.2 lines 3509–3521; §16.2 lines 3691–3694, 3727–3729;
§16.4 line 3833.

**Fix (wire-free):** a shell verb that awaits full acknowledgment
(`SendStream::acked().await`, or `Connection::flush().await` covering
message streams), riding the existing `StreamFinished` core event. At
minimum, a fourth entry in STORIES.md's documentation-obligations list —
this is a sharper trap than the three recorded, because it corrupts data
rather than returning a surprising error.

**Confidence:** certain (composed from explicit spec text).

### FAB-3 — MAJOR — dropping `Connecting` is unspecified, and the obvious use (an app-side connect timeout) bricks redial for up to 90 s

**Expectation:** `HANDSHAKE_GIVEUP` is 90 s and there is no configurable
connect deadline, so every real application will wrap `connect()` in
`tokio::time::timeout(...)` / `select!` — and expect dropping the
`Connecting` future to cancel the attempt.

**Actuality:** §16.2's drop-semantics paragraph (lines 3727–3739)
enumerates staged objects, `Connection`, `SendStream`, `RecvStream`, and
all-handles — **`Connecting` is absent**. Nothing says whether dropping
it cancels the pending handshake, stops the §5.5 retransmit train, or
what happens if msg2 lands afterwards (a connection installed that no one
holds — closed? reaped at 25 s per the §7.4 install pin? emitting
keepalives?). Meanwhile §16.1 (lines 3620–3622) makes *"an in-flight
outbound connect"* return `ConnectError::AlreadyConnected` to any new
`connect()`. If drop does **not** cancel, the composition is nasty: app
times out at 10 s, drops `Connecting`, retries — and gets
`AlreadyConnected` on every retry until the invisible original gives up
at 90 s. That is the S3a trap's evil twin, and unlike S3a there is no
`close()` escape hatch — there is no handle left to close.

**Location:** §16.2 lines 3727–3739 (the enumeration that omits it);
§16.1 lines 3620–3622; §5.5 lines 789–820; STORIES.md S3a lines 60–64.

**Fix (wire-free):** one sentence in §16.2's drop paragraph: dropping
`Connecting` cancels the pending (retransmits stop, the pending-index
entry and the static's in-flight reservation are released; a msg2 racing
the drop is dropped). Then S3a's doc obligation should mention it.

**Confidence:** certain that it is unspecified; likely that real
consumers hit the composition.

### FAB-4 — MAJOR — S25 (send-syscall errors) has no landing zone: the error taxonomy is ratified closed, has no I/O variant, and the `Wire` trait is never specified

**Expectation:** S25 — the one story with a filed consumer complaint —
promises *"a failing send syscall — `ENETUNREACH`, `EMSGSIZE`, a dead
interface — surfaces as itself, not as a bare timeout 25 s later."* A
reader expects to find the type that carries it and the seam it crosses.

**Actuality:** §18.1 opens *"Closed and normative: every variant this
specification names appears here exactly once"* (lines 4275–4277) — and
no variant anywhere in it can carry an `io::Error` or an errno. Every
send-adjacent result (`DatagramError`, `WriteError`, `MessageError`,
`ConnectError`) is a closed enum without an I/O arm. The `Wire` trait —
S25's own named anchor — is mentioned exactly once, as a phrase (*"The
`Wire` trait seam … is the driver's I/O boundary"*, §16.3 line 3749),
with no methods, no error type, no statement of what the driver does
when a send returns an error. So the acceptance criterion is not
implementable, and not testable, against the ratified spec: fixing the
carried defect requires reopening a section ratified as closed. There is
also a *routing* question the spec must answer, not the implementer:
`ENETUNREACH` on one destination is a per-connection fact — does it
surface on the next verb call, as an event (FAB-1 again), or as
`ConnectionLost::{new variant}`?

**Location:** §18.1 lines 4275–4342; §16.3 line 3749; STORIES.md S25
lines 344–353.

**Fix (wire-free):** a ruling that (a) defines the `Wire` trait's send
contract including its error path, and (b) amends §18.1 with the
carrier(s) — before implementation planning, since S25 is in the
acceptance set.

**Confidence:** certain.

### FAB-5 — MAJOR — messages and uni streams share one incoming supply; mixing the two modes on one connection is incoherent for the receiver, and nothing consumer-facing says so

**Expectation:** S16 presents messages as a primitive ("a user can send a
single-shot message") alongside S12/S13 streams. A reader will design an
app that uses `send_message` for control traffic *and* `open_uni` for
transfers on the same connection — the API shape invites it.

**Actuality:** §9.8 (lines 2843–2848): *"The two receive verbs draw from
the same incoming-uni supply … Which mode consumes a given stream is the
receiving application's choice, **invisible on the wire**."* There is no
discriminator. A receiver running `accept_uni()` and `recv_message()`
concurrently gets nondeterministic assignment: `accept_uni` can claim a
stream the sender meant as a message (which then never surfaces from
`recv_message`), and vice versa. The design is deliberate and internally
sound — but the consequence, *"a connection must commit to one uni-mode,
or the application must multiplex in-band,"* appears in no story, no ⚠
CHECK, and none of the three documentation obligations. Note the sender
half is also asymmetric in a way that invites the mix: `open_bi` is safe
alongside messages (bidi streams are a separate supply); it is exactly
`open_uni` + `send_message` that collides.

**Location:** §9.8 lines 2840–2848; STORIES.md S16 lines 244–247 (no
warning), S13 lines 221–226.

**Fix (wire-free):** add it as documentation obligation #4 with the
recommended pattern ("bidi for streams when you also use messages", or
in-band tagging). Optionally a builder/connection-level mode assertion in
the implementation.

**Confidence:** certain (the mechanism is explicit; the missing warning
is the finding).

### FAB-6 — MINOR — `Contested { under_probe: bool }`: the emission moment is ambiguous, and the `false` arm collides with a real state the spec defines

This is the ruling-45 integration check requested. The event is a good
addition and the at-most-once-per-mark bound is clean, but two edges:

1. **When is `{ under_probe: true }` emitted?** §16.4 (lines 3858–3860)
   says *"when a basis-`None` refusal marks the connection **and** the
   probe goes out"* — one moment. But §7.5 (lines 2271–2275) and §16.5
   (lines 3913–3916) are explicit that the mark and the probe's
   transmission can be **separated**: a probe §7.3's amplification budget
   will not yet admit leaves the mark *"pending rather than failed"*, and
   the deadline arms only at transmission. (§15.4's contested row, line
   3567, says "the probe's one PING, at the mark" — the same conflation.)
   During a mark-pending gap, has the event fired? Two implementers will
   answer differently, and the answer is app-observable (an event with no
   armed deadline behind it, or a refusal with no event yet). The gap is
   reachable exactly when the connection just roamed to an unvalidated
   address — not an exotic corner for a mobility-first product.
2. **The bool's shape.** `under_probe: false` means "mark cleared" — but
   it *reads* as "contested, not under probe", which is precisely the
   pending state in (1). A reader skimming the enum will mis-map the two
   variants onto the three actual states (marked-pending, probing,
   cleared). Since the event is a signal, two variants
   (`Contested`/`ContestCleared`) or a small state enum would carry the
   same information without the collision. `bool` needs no *more*
   information to be actionable (the scheduler only needs set/cleared +
   `Closed` for the death arm — that part of the design is right); it
   needs less ambiguous *naming*.

**Location:** §16.4 lines 3838, 3856–3875; §7.5 lines 2271–2278; §16.5
lines 3913–3921; §15.4 line 3567.

**Fix (wire-free):** rule the emission moment (recommend: at the probe's
transmission, matching the deadline; one sentence noting the mark may
precede it under budget), and rename the variants or the field. Enum
shape is pre-implementation-cheap and post-release-expensive.

**Confidence:** certain on the emission ambiguity; the shape point is
judgment.

### FAB-7 — MINOR — `AcceptError::Stale` is four different situations demanding four different application responses

§18.1 (lines 4303–4313) folds into one variant: (a) nothing parked for
the static; (b) the basis-`None` contested refusal; (c) candidate not
strictly newer than the basis; (d) tie-break winner on a PENDING static —
where the correct responses are respectively "await the next Intro",
"watch the contested verdict" (FAB-1), "await a newer Intro", and **"do
nothing — your own `connect()` is about to complete"**. Case (d) is the
sharp one: an application that treats `Stale` as "re-dial" fires a
`connect()` that hits `AlreadyConnected` (in-flight outbound). The
information to disambiguate exists app-side (own pending-connect
bookkeeping, plus the Contested event when it lands), but nothing tells
the reader they must assemble it. §15.4 (lines 3591–3594) already
concedes the variant under-informs for (b).

**Location:** §18.1 lines 4303–4317; §15.4 lines 3591–3594.

**Fix (wire-free):** either sub-variants on `Stale` (taxonomy ruling), or
— cheaper — a normative decision table in the rustdoc for `accept()`.
Deserves to join the documentation-obligations list either way.

**Confidence:** certain.

### FAB-8 — MINOR — early sends (§16.9) are a specified capability the shell surface cannot reach

§16.9 (lines 4007–4024) specifies pre-establishment opens, writes,
messages and datagrams as "ordinary work" queuing in the connection core,
and §16.2 annotates `id() -> Option<StreamId>` with *"None before
establishment"* (lines 3679, 3684) — implying the application can hold
stream handles before establishment. But the shell offers no path to
them: `connect()` returns a `Connecting` future (line 3650), and the
application first touches a `Connection` after it resolves — at which
point it is established. Either there is a missing surface (a way to get
the `Connection` handle eagerly from `Connecting`, quinn-0-RTT-style — a
genuinely useful capability for a redial-heavy product, and no story
covers it) or the `id() = None` annotations and half of §16.9 describe
unreachable states. An implementer will guess; the two guesses produce
different public APIs.

**Location:** §16.9 lines 4007–4024; §16.2 lines 3650, 3679, 3684.

**Fix (wire-free):** rule it one way: expose early handles (and add the
missing story), or scope §16.9 to core-level test drivability and delete
the shell-facing implications.

**Confidence:** likely (I may be missing an intended mechanism, but it is
not written).

### FAB-9 — MINOR — no metrics surface at all: everything an adapting application needs is trace-only

§18.2's four trace targets are the entire observability contract, and
they are **logs** — an operator contract, not a program-readable one.
The shell exposes no RTT, no loss/retransmit counts, no cwnd, no
`bytes_in_flight`, no datagram-queue drop counters (explicitly "core
state", line 3884, surfaced only via `slither::frames`). A consumer
sending unreliable datagrams — the class of traffic that must adapt its
rate — cannot observe the path at all; bubble-engine cannot answer "is
this link bad?" except by measuring its own echoes. No story covers
metrics; S24's test infrastructure would want the same numbers. This is
the one hunt-list area ("observability and metrics") where the answer is
simply: absent.

**Location:** §18.2 lines 4347–4357; §16.2 accessor list lines
3668–3673; §16.4 line 3884.

**Fix (wire-free):** a `Connection::stats()` snapshot accessor
(shared-cell read, consistent with §16.8) — or an explicit §19 deferral
naming it, so its absence is a decision rather than a gap.

**Confidence:** certain the gap exists; suspicion on how soon it bites
bubble.

### FAB-10 — MINOR — suspend/resume and clock steps are unaddressed, and the phone-asleep case is this product's home turf

Nothing in the spec mentions device sleep. Two concrete consequences:

1. **Resume blindness.** On the platforms bubble targets, `Instant` is
   built on clocks that stall during sleep. Close the lid mid-connection
   for a minute: the peer reaps us at its 25 s liveness; we resume with
   `last_authenticated_recv` apparently fresh, believe the connection
   live (`is_established() == true`), and write into a black hole until
   our own arming-send + `DEAD_TIMEOUT` fires — up to ~25 s of dead air
   *after* wake, then `TimedOut`, then redial. There is no
   app-invocable "probe now" verb (the contested-probe machinery exists
   but only §6.4 can trigger it), so an app with a platform wake signal
   can only close-and-redial on speculation.
2. **Wall-clock regression across a peer restart.** The initiation
   timestamp is the one wall-clock read (§5.3 lines 715–717). A peer
   that restarts with a stepped-back clock (dead RTC, post-reboot NTP)
   presents timestamps older than our guard/basis records: its
   handshakes are refused — surfacing to *our* application as
   `AuthError::Replay`, a near-security signal, for a benign cause —
   until our zombie dies (≤ 25 s) and the orphaned guard entry ages out
   (§17.1's `INTRO_TTL`-scale aging). The system **self-heals inside the
   peer's 90 s `HANDSHAKE_GIVEUP`** — genuinely good design — but
   neither the delay nor the misleading `Replay` reading is written
   anywhere a consumer will look, and S20 promises restart-reconnect
   unconditionally.

**Location:** §16.5 lines 3907–3910; §5.3 lines 713–720; §17.1 lines
4056–4068, 4099–4104, 4157–4158; STORIES.md S20 lines 292–297.

**Fix (wire-free):** (a) a short "sleep and clocks" note in the spec or
crate docs covering both behaviours; (b) consider an app-invocable
liveness probe verb (the PING frame and the ack-eliciting machinery
already exist — no wire change) as the resume story; (c) one sentence in
S20 bounding the clock-regression case.

**Confidence:** likely (platform clock behaviour varies; the spec's
silence is certain).

### FAB-11 — MINOR — story-set hygiene: S16 names a constant that does not exist, anchors the wrong section, and the stories omit the one datagram bound an app codes against

- S16 (STORIES.md lines 244–247): "a message up to `MAX_MESSAGE` in one
  call" — no such constant. The spec's bound is `MESSAGE_RECV_MAX` =
  `INITIAL_MAX_STREAM_DATA` = 262 144 B (§9.8 line 2852). Since the
  story notes bubble-engine "cares about the exact bound," the name and
  number should be exact. Its anchor "§11" is the datagram section;
  messages are §9.8. (It is also the only paused-clock-eligible story
  with no paused-clock note.)
- The stories' constants header (lines 26–29) lists `MAX_DATAGRAM` 1200
  and `MAX_PLAINTEXT` 1170 but not **`MAX_DATAGRAM_PAYLOAD` = 1169**
  (§11.2, constants table line 4858) — the actual `send_datagram`
  acceptance bound. A reader sizing datagrams from the stories picks
  1170 or 1200; both are `DatagramError::TooLarge`.
- Trivial: S24 (line 341) lists the timer family without the 25 ms
  `MAX_ACK_DELAY` member that §16.10 (line 4030) includes.

**Fix:** editorial, three lines. **Confidence:** certain.

### FAB-12 — NOTE — endpoint shutdown is silent toward peers, and S26's "cleanly" will be read as stronger than it is

§15.4's endpoint-dropped row (line 3573): nothing transmitted; peers
discover by liveness, ≤ 25 s. So graceful shutdown is *per-connection,
manual, ordered*: close every connection (each an await), then drop the
endpoint — and the drop-order sensitivity is real, since dropping a
`Connection` handle with the driver alive sends CLOSE (lines 3728–3729)
while dropping everything at once sends nothing (line 3738–3739): field
order in an application struct decides whether peers are told. S26
("tears everything down cleanly") is honest about its local claims but a
reader will assume peers are informed. A `close_all()`-style endpoint
verb, or one documentation paragraph on shutdown ordering (including
whether the final CLOSE datagrams are guaranteed onto the socket if the
endpoint is dropped immediately after `close().await` resolves — "sealed"
is not "transmitted"), closes it. Wire-free.

**Confidence:** certain on the mechanics; the reader-expectation gap is
judgment.

### FAB-13 — NOTE — S17's "stalls its own stream, not the connection" is true only up to four stalled streams

`INITIAL_MAX_STREAM_DATA` (256 KiB) × 4 = `INITIAL_MAX_DATA` (1 MiB): an
application that holds four unread streams pins the whole connection
window and stalls every sender-side stream. Standard QUIC economics, not
a defect — but the story's acceptance states the per-stream isolation
unconditionally, and a test written to it will pass while the four-stream
composition fails the claim's spirit. One qualifying clause in S17 fixes
it. (The abandoned-handle case is already handled correctly — §16.2's
RecvStream-drop rule credits the connection window back; this note is
about held-but-unread streams only.) **Confidence:** likely.

### FAB-14 — NOTE — S19's "we change interface" silently assumes a wildcard-bound socket

Our own Wi-Fi→cellular move works because the kernel re-routes a
wildcard-bound socket and the source address changes; the peer re-homes
on our next authenticated packet. A socket bound to a specific interface
address has no recourse — the `Wire` seam (undefined, FAB-4) has no
rebind operation. One doc line ("bind wildcard") makes S19's promise
true by construction. During the move gap, sends fail at the syscall —
which is FAB-4/S25's surface, another reason that ruling comes first.
**Confidence:** likely.

---

## Checked and fine (so their absence from the findings is a verdict, not an oversight)

- **Memory pressure / state ceilings:** §17.5 bounds every per-connection
  commitment honestly (including the admission-exemption overshoot);
  live-connection count is explicitly application-governed. Correct
  posture for an app-driven accept model.
- **Connection limits:** the pull-model accept means the application *is*
  the admission gate; no protocol cap is needed and none is missing.
- **IPv4/IPv6:** the spec is address-family-agnostic end to end
  (`SocketAddr` everywhere, roaming included); dual-stack is the
  app-supplied socket's business. Nothing to fix beyond FAB-14's doc line.
- **NTP steps while running:** the §17.2 endpoint-global strictly-greater
  forcing absorbs backward wall-clock steps within a process lifetime;
  only the restart composition (FAB-10.2) is visible.
- **S5's self-sustain claim:** verified against §7.4/§7.5 — one marking
  exchange does bootstrap the mutual keepalive dance; the story's wording
  matches the mechanism.
- **Story testability at large:** with the exceptions named above (S25
  untestable as specified; S11/S18's "application observes" testable only
  at core level until FAB-1 is ruled; S16's constant), the paused-clock
  claims hold — the acceptance criteria are genuinely drivable through
  `core::{Endpoint, Connection}` + `FlakyWire`, and Appendix B already
  scripts the hard ones (the ruling-41 high-water-mark regression is a
  model test obligation).

## What is genuinely good

Named so the maintainer can tell real problems from noise — none of this
should be disturbed:

- **The staged accept ladder** (0/1/2/4 DH with a suspendable, owned,
  `#[must_use]`, non-`Clone` mid-state) is the best thing in the design:
  the cost accounting is explicit, the claimed/proven distinction is
  carried in types, and the test bars ("a counting DH provider asserts
  `dhs == 0`") make the claims falsifiable.
- **The liveness model** (§7.4): the marking/arming split, the install
  pin, and the "what the death clock does *not* measure" paragraph are
  exemplary — the spec states its own weakest point (mere authenticated
  receipt) and then builds the one stronger mechanism exactly where that
  weakness is exploitable.
- **The contested probe** (rulings 36/41/43): the high-water-mark
  predicate, the no-re-arm collapse as a security property, the honest
  cost bound replacing a false one, and the declined alternatives
  recorded with reasons. This is how security rulings should be written.
  FAB-6 is a naming-and-timing polish on top of a sound mechanism.
- **The teardown matrix** (§15.4): every death, what is transmitted, both
  sides' views, in one table — the single most consumer-useful page in
  the document. The FAB findings above would mostly have been prevented
  if the shell surface had an equivalent table.
- **The pull model / no-unbounded-queue discipline** (§16.4, §16.8,
  §10.6): reliable data is never droppable, backpressure is by
  retention, and the shell is deadlock-free by construction.
- **Kernel-free drivability** (§16.10, S24) with the explicit lateness
  bound `L`: the whole timer family in virtual time is what will make
  every finding above cheaply testable once ruled.
- **The honesty clauses** (§17.1's eviction consequences, §6.9's stated
  ratios, §7.4's beacon-into-unvalidated-anchor bound): the document
  consistently states residual weaknesses instead of hiding them, which
  is rare and worth protecting through future revisions.
- **The stories document itself**: S3's three-way split, the ⚠ CHECK
  convention, and the carried-forward section (recording the INTRO_TTL
  trade *before* it bites) are exactly the right shape — the findings
  above are gaps in its coverage, not flaws in its method.

---

*Summary for the ruling queue (all wire-free): FAB-1 (event surface),
FAB-2 (delivery-wait verb + doc obligation), FAB-3 (Connecting drop =
cancel), FAB-4 (Wire trait + I/O error carrier), FAB-5 (uni-mode doc
obligation), FAB-6 (Contested emission moment + naming). FAB-7–14 are
docs/editorial or deliberate-deferral candidates.*

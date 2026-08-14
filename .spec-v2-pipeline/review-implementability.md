# Implementability review — SPEC-v2-DRAFT.md (phase 1)

Reviewer lens: could a competent Rust developer build exactly what the draft
says, against hiss 0.3.1 + Appendix A's proposed additions and the frozen v1
wire, with no contradiction, no missing piece, no impossible requirement?
Cross-checked against `design-choices.md`, `research-hiss-api.md`,
`research-actor-inventory.md`, the real slither source
(`src/{endpoint,handshake,session,recovery,frame,wire}.rs`), and hiss 0.3.1's
source (`src/noise/{datagram,cipher_state}.rs`, `hiss-macros/src/codegen.rs`).

Severity: **BLOCKER** = cannot be built as written · **MAJOR** = ambiguity an
implementer would have to guess · **MINOR** = smaller gap, easily fixed ·
**NIT** = cosmetic.

---

## 1. [BLOCKER] `accept()`'s returned `core::Connection` vs a later `Install` event — genuinely undecidable from the text

**Section/line**: §6 "The concrete surfaces", `core::Endpoint::accept()`
(line 436-437) and `EndpointToConn::Install` (line 452-455); contrast with
§6's `connect()` (line 424-425) and §5 "Simultaneous open" (line 315-326).

**Problem**. `connect()` returns `(ConnectionId, core::Connection)` in a
**connecting** state (no session yet — §6 "queued sends... land in the
ordinary Leg 2 send queue"); the session only arrives later, out-of-band,
via `poll_output()` → `EndpointOutput::ToConnection(id, EndpointToConn::
Install{initial:true, session})`, which the shell must route to that same
connection's `handle_endpoint_event`. `accept()` has the *identical* return
type — `Result<(ConnectionId, core::Connection), AcceptError>` — but by the
time it is called all 4 DHs are already paid and msg2 is already queued for
transmit (§4's table: "`accept()` → `Connection`... an established
connection"). The draft never states whether:

  (a) the `core::Connection` `accept()` returns is **already established**
      (session baked in at construction) and `poll_output()` must **never**
      also emit `ToConnection(id, Install{...})` for that id, or
  (b) `accept()` returns a connection in the **same bare/connecting shape**
      as `connect()`'s, and a **subsequent** `Install{initial:true, ...}`
      event, symmetric with the connect path, is required to actually
      establish it.

v1 offers no tie-breaker: v1 has no `Connection` *object* at all — `on_init`
(responder) and `on_resp` (initiator completion) both funnel through the
*same* `install_session` function (`src/endpoint.rs:1038-1067`), so v1 never
had to decide "does construction-time-of-the-handle differ from
installation-time" in the first place. Both readings (a) and (b) are
independently plausible and each has a distinct, real failure mode if the
implementer guesses wrong:

  - Under reading (a), if the shell naively routes *every* `ToConnection`
    event (including one an implementer might still emit for symmetry) to
    `handle_endpoint_event`, the connection is installed **twice** — timers
    re-armed twice, and (worse) `Established`/resolution semantics that are
    contracted to fire "exactly once" (§5, "edge-triggered exactly once")
    could double-fire.
  - Under reading (b), if the implementer (reasonably, mirroring `connect()`)
    assumes a later `Install` event is needed but the endpoint core never
    emits one for a **freshly accepted** connection (since nothing in the
    endpoint's `accept()` contract promises it), the connection is **never
    established** at the object level: `is_established()` stays false,
    timers never arm, `send`/`recv` never work. Silent, hard to diagnose.

**Fix**. State explicitly, in §6, one of: "`accept()`'s returned
`core::Connection` is fully established; `poll_output()` never emits an
`Install` for a `ConnectionId` that has not yet been surfaced to the shell
via `connect()`'s connecting-state return" — or the reverse, with an explicit
guarantee that `accept()` is *always* immediately followed (within the same
drain) by exactly one `ToConnection(id, Install{initial:true, ...})` for the
same id. Either is buildable; the draft currently permits both and rules
out neither.

---

## 2. [MAJOR] Is `HandshakeFailed` ever delivered to `core::Connection::handle_endpoint_event`?

**Section/line**: §6, `EndpointToConn::HandshakeFailed` (line 455) and
`core::Connection::handle_endpoint_event(&mut self, now, ev: EndpointToConn)`
(line 462); §5 "Simultaneous open" (line 320-326).

**Problem**. `handle_endpoint_event` is typed to take the *whole*
`EndpointToConn` enum, so its signature implies `core::Connection` must
handle both `Install` and `HandshakeFailed`. But the only narrative use of
`HandshakeFailed` is shell-level: "`Connecting` resolves... on the first
`Install{initial:true}` or `HandshakeFailed`." If `HandshakeFailed` is meant
to resolve `Connecting` **directly at the shell**, bypassing the connection
core entirely (plausible: the connection was never established, so there is
nothing meaningful for the core to do with it), then `handle_endpoint_event`
should never actually receive that variant in practice — and its signature
is misleading. If it *is* meant to reach the core (so the core can reach a
defined terminal state before the shell discards the object), the draft
never says what that transition produces: `ConnectionLost` has no variant for
"never established" (`TimedOut` is defined as liveness specifically,
`RekeyFailed` is the 180s payload backstop — neither fits an initial-connect
give-up).

**Fix**. State explicitly whether `HandshakeFailed` is ever handed to
`core::Connection`, and if so, what it produces (a terminal state? a specific
`Closed(...)` variant?) and whether the shell must *also* directly resolve
`Connecting` from the endpoint-level event, or *only* by observing the
connection's own subsequent output.

---

## 3. [MAJOR] The staged-verb error enums have no "driver is gone" variant

**Section/line**: §4 "Errors" paragraph (line 172-179); §10 error taxonomy
(line 758-777).

**Problem**. Each staged verb (`read_identity`, `authenticate`, `accept`) is
"a driver round-trip" (§4, §6 C6) — the handle sends a request and awaits a
reply from the single `!Send` driver task. If that driver task has already
exited or panics mid-flight (a real, if rare, occurrence — a socket error,
an unhandled internal bug), the awaiting future's reply channel closes
without a reply. `IntroError::{Expired, Superseded, Internal, Malformed}`
and `AuthError::{Replay, HandshakeFailed, Expired}` have **no variant** for
this. v1 already anticipated exactly this class of failure —
`SlitherError::EndpointClosed` (`src/endpoint.rs`, actor inventory §1.10) —
and v2 carries the *concept* forward for established connections
(`ConnectionLost::EndpointDropped`, §10) but drops it for the pre-connection
staged path. An implementer is left to either panic/unwrap on channel
closure, or silently misuse an unrelated existing variant (e.g. reporting
`Malformed` or `Expired` for a condition that is neither).

**Fix**. Add a variant (e.g. `IntroError::EndpointDropped` /
`AuthError::EndpointDropped`, and whatever `AcceptError` ends up being, see
finding 13) covering driver-loss, mirroring `ConnectionLost::EndpointDropped`.

---

## 4. [MAJOR] "Keepalive evaluation precedes teardown collection" inverts v1's actual order

**Section/line**: §6 "Equal-deadline priorities" (line 507-510).

**Problem**. The draft states this ordering rule as a restatement of "v1's
tick ordering." But v1's actual code (`src/endpoint.rs:904-925`, per
`research-actor-inventory.md` §1.1) is: *"iterate sessions: `is_dead` ⇒
collect for teardown **and continue**; **else** `should_keepalive` ⇒ seal
empty Data."* Teardown is checked **first**; if the session is dead, the
keepalive check is skipped outright (`continue`) — i.e. **teardown wins
over keepalive**, not the reverse. This is not a hypothetical edge case:
`is_dead` requires `now - last_send >= DEAD_TIMEOUT` (15s), and
`should_keepalive`'s persistent branch requires `now - last_send >=
persistent_interval`; with the **recommended** `PERSISTENT_KEEPALIVE = 25s`
(§8, unchanged from v1), any connection idle long enough to owe a persistent
keepalive (25s since last send) has *already* satisfied `is_dead` (15s) —
the overlap is reachable with the documented default configuration, not a
corner case. An implementer following the draft's literal words would send
a keepalive moments before/instead of tearing the connection down —
diverging from the v1 behaviour this section claims to preserve.

**Fix**. Correct the bullet to "teardown collection precedes keepalive
evaluation; a session already collected for teardown owes no keepalive,"
matching `src/endpoint.rs:904-925`.

---

## 5. [MAJOR] Deadlock-freedom of the shell is not stated as an invariant, only demonstrated for one path

**Section/line**: §6 "The concrete surfaces" (`Connection` accessors, line
470); §3 "Driver and handle lifetimes"; §6 C6 receive-backpressure (line
675-693).

**Problem**. The task-relevant hazard — "handle awaiting driver while driver
awaits a full handle channel" — is concretely and correctly avoided for
*one* path: the receive-message buffer's shed policy (§8) is explicitly
non-blocking (decrypt → check → maybe-mark-and-deliver **or shed**), so the
driver never blocks pushing a delivered message. But this is the *only*
driver→handle path the draft analyses. `Connection`'s other accessors —
`remote_static()`, `remote_address()`, `session_id()`, `is_established()` —
are listed with no indication of whether they are synchronous local reads
(e.g. of a shared cell the driver updates) or *also* driver round-trips
(oneshot request/reply, like the staged verbs). If an implementer makes
`remote_address()` (which must reflect roaming, per §8) a round-trip through
the same bounded command channel used for `send`/staged verbs, and that
channel is momentarily saturated by unrelated traffic, the accessor call
blocks — this is not by itself a deadlock, but the draft gives no general
rule ("the driver never performs a blocking send toward a handle") that
would let an implementer *rule out* constructing one when adding a new
handle-facing surface later. The property the task asks to verify
(deadlock-freedom) is demonstrated for one instance, not established as an
architectural invariant.

**Fix**. State the general rule explicitly: all driver→handle delivery is
either (a) via a bounded channel with an explicit, non-blocking shed policy
(as specified for the receive buffer), or (b) via a single-slot / oneshot
reply that cannot block a sender. State which category each accessor and
each staged verb's reply falls into.

---

## 6. [MAJOR] `send_unreliable`'s `seq` must share the reliable path's namespace — unstated, and unsupported by `Recovery`'s current API

**Section/line**: §9 "`send`, `send_unreliable`, and the `Reliable`
boundary" (line 711-733); cf. `src/recovery.rs::queue_message` and
`DataFrameHeader` (`src/frame.rs:130-147`).

**Problem**. The wire's `DataFrameHeader` (`type(1) ‖ seq(8) ‖ length(2)`)
carries no reliable/unreliable flag — confirmed from `src/frame.rs`. The
draft states receive-side dedup is unchanged ("dedup and exactly-once
surfacing apply as normal"). For that dedup to work correctly, an unreliable
send's `seq` **must** be drawn from the *same* monotonic space as
`Reliable::send`'s — otherwise a reliable message and an unreliable message
issued around the same time can collide on `seq`, and the receiver's
`delivered_floor`/`delivered_sparse` dedup (keyed purely on `seq`) would
silently treat one as a duplicate of the other and drop it. The draft never
states this sharing requirement, and `Recovery`'s current API
(`queue_message`) has no method that allocates a `seq` **without** also
inserting into `outstanding`/`to_send` (i.e., no "allocate but don't track"
primitive) — a real, if small, method needs adding. §13's "New obligations"
list (stage-0 queue, hint routing, pacing, guard eviction, index re-draw,
receive-buffer shed, plan-then-commit, `AlreadyConnected`) does not mention
a test for reliable/unreliable interleaving.

**Fix**. State explicitly that `send_unreliable` draws from the same `seq`
counter as `Reliable::send` (just skipping the retransmission-queue insert),
add the corresponding `Recovery` method to the surface list, and add an
interleaving test to §13's obligations.

---

## 7. [MAJOR] The check-vs-mark shed rule leaves replay detection off during backpressure

**Section/line**: §8 "Receive backpressure" (line 675-693); evidence:
`src/session.rs`'s `ReplayWindow::admit` (lines 72-104), which today
performs check-and-mark atomically.

**Problem**. The specified shed sequence is: decrypt → replay **check**
(read-only) → if fresh, update liveness/roaming → **only if the buffer has
room**: replay **mark** + deliver. This is buildable (the split is
mechanical — `check` and `mark` can share the existing `admit` logic). But
its consequence is not analysed: while the buffer stays full, a counter that
was checked-fresh-but-not-marked stays **unmarked indefinitely**. If that
exact packet (or an attacker's captured copy of it) is redelivered while the
buffer is *still* full, `check` will find it fresh **again** (its bit was
never set), pass AEAD (it is a byte-identical, validly-authenticated replay
of a real packet), and *again* update liveness/roaming — repeatedly, for as
long as backpressure persists. This is not a confidentiality/integrity
break, but it is a real, reachable divergence from v1 (where every `open()`
call marks unconditionally, so an immediate replay is always rejected) that
the draft doesn't flag: under sustained backpressure, replay protection is
effectively suspended for the shed tail of the window, and an attacker who
can capture one valid packet can keep the connection's liveness clock alive
indefinitely by replaying it.

**Fix**. Either accept this explicitly as a documented, bounded exposure
(backpressure is itself transient and bounded by `RECV_BUFFER` draining), or
require that `check` mark the counter as "provisionally seen" (a third
state, distinct from the `mark`-for-delivery bit) so a repeat during the
*same* backpressure episode is still rejected as a duplicate before the
liveness/roaming update. The draft currently does neither and doesn't
acknowledge the gap.

---

## 8. [MAJOR] Appendix B's "survives verbatim, no harness change" claim conflicts with the allow-list's dissolution

**Section/line**: §13 "Pins that survive verbatim" (line 865-873); §4/§10
("the allow-list closure is turned inside out" / "allow/revoke... dissolved:
policy = the application's staged decision").

**Problem**. Spot-checking the "survives verbatim" list against
`src/handshake.rs`:

| Test | Calls `accept_init(..., allow: &HashSet<[u8;33]>)` directly? |
|---|---|
| `golden_wire_is_byte_identical_to_the_pre_migration_driver` | **yes** |
| `msg_sizes_match_the_wire_pins` | **yes** |
| `tampered_payload_fails_then_clean_msg1_succeeds` | **yes** |
| `responder_dh_cost_is_staged` | **yes** — the whole test's premise is exercising the allow-list-closure's DH-staging property (empty allow-list vs populated allow-list) |
| `rekey_transform_kat`, `protocol_name_is_pinned`, `timestamp_is_not_on_the_wire`, `timestamp_guard_rejects_non_greater` | no — genuinely independent |

`accept_init`'s entire design **is** the mechanism §4 replaces ("the
allow-list is the read's verification closure" — its own module doc,
`src/handshake.rs:24-43`); its signature takes an explicit
`allow: &HashSet<[u8;33]>`. The draft never states whether
`handshake.rs`'s current one-shot functions (`accept_init`, `RespAccept`,
`build_init`, `complete_init`) are **retained** unchanged as a
now-legacy/internal layer the new `core::Endpoint` no longer calls (in which
case these four tests do survive, as now-slightly-vestigial unit tests of
dead-in-production code — a trade-off worth stating), or **removed/replaced**
by the split-read-based staged primitives the core actually needs — in which
case these four tests, including the phase's own stated "verdict test"
(`golden_wire_is_byte_identical_to_the_pre_migration_driver`, named as such
in §1 and §13), cannot compile unchanged and must be rewritten to call the
new primitives (their *assertions* — the golden hex — can stay identical;
their *call sites* cannot).

**Fix**. State explicitly whether `handshake.rs`'s current allow-list-shaped
public functions are retained (dead-code-adjacent) or removed; reclassify
these four tests accordingly (either "survives, but now exercises an
internal-only compatibility layer" or "harness rewrite: same assertions, new
call site").

---

## 9. [MINOR] The same "does the old single-call API survive as a wrapper" question recurs for `ReplayWindow::admit` and `Recovery::next_packet`

**Section/line**: §13 (line 865-873) vs §6 "Plan-then-commit sealing" and §8
C6's check/mark split.

**Problem**. `session::tests::window_*` (4 tests) call
`ReplayWindow::admit(counter)` as one combined call; §8 requires splitting
`admit` into `check`+`mark`. Separately, `recovery::tests` (9 tests)
exercise `Recovery::next_packet`, which — read from source
(`src/recovery.rs:297-362`) — **already** mutates `to_send`/`transmitted`/
`ack_pending`/`ping_pending` eagerly, before any seal happens; this is
exactly hazard H1 the draft's "plan, seal, then commit" fix targets. Genuinely
fixing H1 requires more than reordering call sites: `next_packet`'s planning
and its mutation need to be **split** into a side-effect-free plan step and
a separate commit step invoked only after a successful seal. Unlike finding
8, this is low-severity because the natural fix — keep `admit`/`next_packet`
as thin wrappers around the new split primitives, for callers (including
these tests) who don't need the split — costs only a few lines and changes
no observable behaviour. But the draft doesn't say this is the intended
resolution, leaving it, like finding 8, to implementer discretion.

**Fix**. Note in §13 that `admit` and `next_packet` are retained as
convenience wrappers over the new split primitives, so the listed unit tests
need no changes.

---

## 10. [MINOR / NIT] `core::Endpoint::new(..., identity: Identity, ...)` uses a trait name where Rust requires a type

**Section/line**: §6 "The concrete surfaces" (line 422).

**Problem**. `Identity` is a trait (`src/handshake.rs:158-172`), not a
concrete type; `identity: Identity` as written does not compile (needs
`impl Identity`, or `core::Endpoint` to be generic, `core::Endpoint<I:
Identity>`). This matters beyond naming: the endpoint core's mid-state
storage (`HashMap<IntroId, IKResponderMsg1Mid<I::Provider>>`, per finding on
hiss fit above) is only well-typed if `core::Endpoint` carries the same `I:
Identity` generic parameter throughout — exactly mirroring v1's actual
pattern, where the generic `Actor<I: Identity, W: Wire>` is monomorphized at
spawn time behind a non-generic `Endpoint` handle (`src/endpoint.rs:237-279`).
The draft's own "normative in shape" disclaimer covers renaming, not adding
a missing generic parameter that changes whether the snippet type-checks.

**Fix**. Show `core::Endpoint<I: Identity>` (or an explicitly generic
constructor `fn new<I: Identity + 'static>(...)`), matching v1's existing
`Actor<I, W>` monomorphize-then-erase pattern.

---

## 11. [MINOR] `Retired` delivery ordering and the guard's pinned→orphan demotion trigger aren't stated as MUSTs

**Section/line**: §6 `ToEndpoint::Retired` (line 483); §7 guard pinning
(line 581-586).

**Problem**. The endpoint needs `ToEndpoint::Retired` (or equivalent) to
free the index route *and* to demote the connection's timestamp-guard entry
from pinned to the bounded orphan LRU (§7: "pinned... while a live
`Connection`... exists"). The draft doesn't state that the shell **must**
forward `Retired` before dropping/forgetting the `Connection` object
(distinct from the documented "all handles dropped ⇒ driver stops, no
teardown at all" case, which is explicitly exempted). If a single
connection among several is closed (`close()`, liveness, or the payload
backstop) while the endpoint and other connections stay alive, and the
shell drops the `Connection`'s bookkeeping before observing/forwarding
`Retired`, the index-route and guard-pin entries leak for the life of the
endpoint — a slow leak inconsistent with §7's stated bound on the guard.

**Fix**. State as a MUST: every terminal `ConnOutput` (a `Closed(...)`
event, or `close()`) is followed, within the same drain, by
`ToEndpoint::Retired`, and the shell must deliver it to
`handle_connection_event` before releasing the connection's shell-side
bookkeeping.

---

## 12. [MAJOR] `handle_timeout`'s idempotency for Loss/PTO depends on an unstated synchronous-sealing assumption

**Section/line**: §6 "`handle_timeout` is idempotent" (line 504-506);
evidence: `src/recovery.rs` `on_pto` (line 518-528), `pto_deadline` (line
501-511), `on_packet_sent` (line 379-390).

**Problem**. For the Keepalive/PersistentKeepalive/Liveness timers,
idempotency falls out naturally (sealing updates `last_send`, which is what
the predicates read — a second immediate call sees the predicate already
false). For Loss/PTO it does not fall out for free: `on_pto()` increments
`pto_count` and marks a probe/retransmit as owed, but the PTO deadline
itself (`time_last_ack_eliciting`) only advances once `on_packet_sent` runs
for the *sealed* probe — i.e. only once "plan, seal, commit" (§6) actually
completes. If an implementation places sealing lazily inside `poll_output()`
(a natural reading, since `Transmit` is a `poll_output`-drained variant)
rather than eagerly inside `handle_timeout()` itself, then two
`handle_timeout()` calls with no intervening `poll_output()` drain would
call `on_pto()` twice, double-incrementing `pto_count` and corrupting the
RFC 9002 backoff schedule (`to_send`'s `BTreeSet` dedups the re-queued seq,
so no double *transmission* results, but the backoff exponent is still
wrong) — a real, if narrow, violation of the stated idempotency guarantee.

**Fix**. State explicitly that "plan, seal, then commit" (including
`on_packet_sent`) executes synchronously within the mutating call that
triggers it (`handle_timeout`, `handle_datagram`, `send`, ...), not lazily
inside `poll_output()`, so that idempotency holds even under a spurious
repeated call before any drain.

---

## 13. [MAJOR] `AcceptError` is never enumerated

**Section/line**: §6, `core::Endpoint::accept()` return type (line
436-437); §4's "Errors" paragraph (line 172-179) covers `IntroError` and
`AuthError` but is silent on `AcceptError`.

**Problem**. `AcceptError` is a normative return type with zero specified
variants anywhere in the draft. An implementer has no guidance on what
`accept()` can fail with — including the case in finding 14 below
(supersession between `authenticate()` returning and `accept()` being
called), and whatever narrow crypto-write failure category exists in the
underlying `write_message_2`-equivalent call.

**Fix**. Enumerate `AcceptError`, at minimum covering `Superseded` and
`Expired` (mirroring `IntroError`), plus the driver-gone case from finding 3.

---

## 14. [MAJOR] `AuthError` is missing `Superseded`, contradicting §4's own supersession rule

**Section/line**: §4 "Errors" paragraph (line 172-179) vs §4 "Supersession"
bullet (line 210-215).

**Problem**. §4's supersession rule states: "the mid-state... is discarded...
and **the next verb on the old chain returns `Superseded`**" — worded
generically, i.e. applicable to whichever staged verb is called next,
including `authenticate()` (Claimed → Proven). But the enum immediately
above it lists `AuthError::{Replay, HandshakeFailed, Expired}` — no
`Superseded`. This is a direct contradiction between the draft's own prose
and its own closed-enum listing: if a fresh initiation supersedes a
`Claimed`'s mid-state while the application is about to call
`authenticate()`, there is no variant in `AuthError` that can carry the
outcome the prose promises.

**Fix**. Add `Superseded` to `AuthError`'s listed variants (and to
`AcceptError`'s, per finding 13, since accept() sits on "the old chain" too).

---

## Verified sound (non-findings, worth recording)

- The split-read (`read_message_1_intro` + `MidRead::complete()`) is a real,
  low-risk hiss-macros change: zero runtime change, reuses the exact
  `support::*` call sequence in the exact order used today
  (`research-hiss-api.md` §2.1-2.4), so byte-compatibility holds by
  construction — Appendix A's plan of record is buildable as described.
- `DatagramSend::next_counter()` is a trivial expose-an-existing-accessor
  change (`CipherState::nonce()` is already `pub(crate)`).
- Nonce exhaustion is genuinely the *only* reachable `encrypt_next` failure
  once slither's existing `MAX_PLAINTEXT` pre-check runs first
  (`hiss::noise::cipher_state::MAX_MESSAGE_LEN = 65535` is far above
  `MAX_PLAINTEXT + TAG_SIZE = 1186`), and `HandshakeError::NonceOverflow` is
  a distinguishable variant — the H1 fix's `Closed(NonceExhausted)` mapping
  is sound.
- Storing per-`IntroId` mid-states (including a `!Send` hardware provider)
  inside the endpoint core is fine under the ruled single-`!Send`-driver
  architecture (§3, §6 C6) — nothing about it needs to cross a thread
  boundary, so `!Send` mid-state living alongside a `!Send` driver task is
  not a genericity problem, just a straightforward `HashMap` keyed by
  `IntroId` (monomorphized the same way v1's `Actor<I: Identity, W: Wire>`
  already is — see finding 10 for the one presentation gap).
- The five named connection timers (`Keepalive`, `PersistentKeepalive`,
  `Liveness`, `Loss`, `Pto`) plus the endpoint's pending
  retransmit/give-up/intro-expiry deadlines are a **complete** accounting
  against v1's actual timer inventory (`research-actor-inventory.md` §1.1);
  `REKEY_AGE`/`REJECT_AGE` are correctly kept as payload-path consults, not
  timers, matching v1's `gate_payload` call sites exactly.
- The empty-send-vs-keepalive distinction is already correctly implemented
  in v1 (`on_data` only invokes the frame layer for non-empty plaintext) and
  the draft's restatement doesn't change it.
- `DataFrameHeader`'s wire layout has no reliable/unreliable flag, confirming
  `send_unreliable` must be a pure sender-side bookkeeping distinction (see
  finding 6 for the one gap in how that bookkeeping is specified).

---

## Item 7 — the writer's "interpretations"

No document among the reviewed inputs (`SPEC-v2-DRAFT.md`,
`design-choices.md`, or the two research reports) contains a numbered
"interpretations 1-7" list or the word "interpretation" at all — this
appears to refer to a summary this reviewer was not given. The closest
analogue in the draft is its eight **[MAINTAINER]**-tagged clauses (lines
28, 67, 127, 196, 372, 591, 608, 806), each a judgement call requiring
sign-off rather than a pure technical derivation. Assessed for
implementability:

| Clause | Verdict |
|---|---|
| No-fallback gating on the hiss split-read minor (line 28) | Implementable — a scheduling/dependency decision, not a code construct; the split itself is confirmed buildable (see "Verified sound") |
| One connection per remote static (line 67) | Implementable — direct generalisation of v1's existing `static_to_conn` map |
| `Intro`/`Claimed`/`Proven` names final (line 127) | Implementable — pure naming |
| Stage-0 dedup key supersedes TODO's `(addr, sender_index)` (line 196) | Implementable — straightforward keyed map with cap/TTL/replace-with-newest |
| Two cores, one poll contract (line 372) | Implementable **in overall shape** (str0m/quinn-precedented), but its concrete surface carries findings 1, 2, 5, 12 above — the architecture is sound, the surface as written is not fully self-consistent |
| Timestamp-guard orphan-LRU eviction consequence (line 591) | Implementable — standard bounded LRU, honestly documented trade-off |
| The swap cuts the old session instantly (line 608) | Implementable — this is *already* v1's live behaviour (`install_session` unconditionally drops the old index/session, confirmed by reading `src/endpoint.rs:1038-1067`); ratifying it for v2 requires no new mechanism |
| Peer-restart seq collision — known limitation, no fix (line 806) | Trivially implementable — a documentation-only obligation |

---

## Verdict: **BUILDABLE-WITH-FIXES**

Summary: 1 BLOCKER, 10 MAJOR, 3 MINOR/NIT.

The wire-frozen surfaces, the hiss split-read proposal, the DoS accounting,
the timer inventory, and the crypto/nonce reasoning are all sound and
buildable as specified — these were the areas most likely to hide a true
impossibility, and none did. The findings cluster almost entirely around
the *new* object-model machinery this draft introduces (the
`core::Connection` installation protocol between `accept()`/`connect()` and
`Install`, the staged-verb error taxonomies, and a few backpressure/
idempotency edge cases at the shell boundary) rather than the protocol core.
None of the findings contradicts the frozen wire or a real hiss capability;
each is fixable with a paragraph of additional specification, not a
structural rework. The worst finding (1): the draft's own concrete type
surface admits two contradictory implementations of how an accepted
connection becomes "installed" — a double-install or a never-installed
connection are equally consistent readings of the text as written, and nothing
in the document breaks the tie.

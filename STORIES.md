# slither — capability stories

> **COMPLETE AND APPROVED 2026/08/14 — 34 stories.** S1–S26 were walked
> one at a time first; S27–S28 followed from rulings 46–47; S29–S30 were
> walked last, after a re-read found the "still open" list entirely
> stale (both items had been ruled in round 8 with no story testing
> them); S31–S33 were approved by ruling 209 (2026/08/16) and S34 by
> ruling 252 (2026/08/17). *(This banner read "30" until 2026/08/17 —
> stale since ruling 209, the 2026/08/17 survey's finding #1; rule 4's
> shape, in the acceptance-criteria document itself.)* **This set is the
> acceptance criteria for `PLAN.md`.**
>
> **APPROVED 2026/08/14.** All 26 stories were walked one at a time and
> approved by the maintainer. **Twenty-five stand as drafted; S11 was
> amended** — "contested" becomes application-visible (ruling 45). Three
> answers given before the walkthrough are folded in: S3's three-way
> split is confirmed as the correct reading of "a second connection
> closes the first"; S7's claimed-vs-proven hazard is agreed; and S18's
> mover obligation is resolved — **the moving peer keepalives**.
>
> The unit is *what a user of the crate can do*, stated so each one is
> directly testable. Six (S1, S3, S6, S7, S12, S20) are the maintainer's
> own; the rest fill the surface around them. Every story names its spec
> anchor in the ratified `SPEC.md`, its DH cost where the staged
> accept makes cost the point, and whether it is drivable on the paused
> clock. **This set is the acceptance criteria for the implementation
> plan.**
>
> "User" means **the application on top of slither** — bubble-engine, or
> any other consumer — not a human. Where a human appears (S8's
> human-in-the-loop) it is said explicitly.
>
> Marked **`⚠ CHECK`** where writing the story surfaced a question the
> spec does not plainly answer, or where the answer may surprise.

Constants referenced: `KEEPALIVE_TIMEOUT` 10 s · `DEAD_TIMEOUT` 25 s ·
`HANDSHAKE_GIVEUP` 90 s · `INTRO_TTL` 15 s · `INTRO_QUEUE_CAP` 1024 ·
`INTRO_MAX_PER_SOURCE` 4 · `PERSISTENT_KEEPALIVE` default 10 s, range
`[1 s, DEAD_TIMEOUT)` · `MAX_DATAGRAM` 1200 · `MAX_PLAINTEXT` 1170 ·
`MAX_DATAGRAM_PAYLOAD` 1169 · `MESSAGE_RECV_MAX` 262 144.

> **REVISED 2026/08/14 after the fable+sonnet review pair** (rulings
> 46–49). Corrections applied: S1's `PeerClosed`/`LocallyClosed` variant
> names, S2's §5.5 anchor and its **fixed 5 s + jitter** retry (not
> exponential), S7's denylist rescoped to the application with the 0 DH
> claim withdrawn, S16's `MESSAGE_RECV_MAX`, S24's attestation gap,
> S25 rescoped to the `Wire` trait plus a trace obligation, and S26's
> trigger corrected to **every handle**. S11 gains ruling 46's real
> delivery mechanism.

---

## A. Connection lifecycle

### S1 — a user can open a connection and close it *(maintainer's #1)*

Dial a peer by static public key and address; get a live connection;
close it cleanly; both sides observe the close.

- **Accepts:** `connect(addr, static)` returns a `Connecting` that
  resolves to a `Connection`. `close(code, reason)` resolves once the
  CLOSE frame is sealed. The peer surfaces
  `ConnectionLost::PeerClosed { code, reason }` with the same code and
  reason (truncated at `CLOSE_REASON_MAX`); our own side sees
  `ConnectionLost::LocallyClosed`.
- **Cost:** 4 DH (initiator side: `es`, `ss`, `ee`, `se`).
- **Anchor:** §16.1, §15.1, §15.2. **Paused clock:** yes.

### S2 — a user can dial a peer that never answers, and be told

- **Accepts:** with no response, `Connecting` resolves
  `Err(ConnectError::TimedOut)` at `HANDSHAKE_GIVEUP` (90 s), not before
  and not never. Retransmissions during the window follow **§5.5**'s
  handshake schedule — a **fixed 5 s interval plus jitter, explicitly
  *not* exponential backoff**. (§13's exponential PTO governs the
  post-handshake data path and does not apply here.)
- **Anchor:** §16.1, §5.5. **Paused clock:** yes — this is a 90 s test
  that runs instantly.

### S3 — a second connection supersedes the first *(maintainer's #2)*

The story splits in three, and the three answers differ. **This is the
most important clarification in this document.**

- **S3a — our own second dial is refused.** `connect()` to a static that
  already has a live `Connection` returns
  `Err(ConnectError::AlreadyConnected)`. The first connection is
  untouched. One connection per static is an invariant, so "the second
  closes the first" is *not* what happens locally.
- **S3b — the peer's fresh handshake replaces ours, if it may.** A new
  handshake from an already-live static drops the old connection and
  installs the new one (the ratchet-only ruling: sessions:connections is
  1:1, restart is structural). But replacement requires a
  `replacement_basis` of `Some(t)` **and** a strictly newer candidate
  timestamp. We hold `Some(t)` only where we were the **responder**.
- **S3c — a connection we dialled refuses replacement.** Where we dialled
  (basis `None`, because msg2 carries no payload and a dialler never
  learns a peer timestamp), the accept is refused `AcceptError::Stale`
  and the connection is marked **contested** — see S11.
- **Accepts:** the old connection's handle surfaces
  `ConnectionLost::Replaced`; in-flight stream data on it is lost;
  the new connection is independent, with fresh stream state.
- **Anchor:** §5.4, §6.4, §16.1, §17.4. **Paused clock:** yes.
- **⚠ CHECK:** S3a means an application that wants "reconnect now" must
  `close()` first, then `connect()`. Worth an explicit example in the
  docs, because "call connect again" is the obvious wrong guess.

### S4 — two peers dialling each other at once end up with one connection

Simultaneous open converges deterministically rather than leaving two
half-connections or going mutually dark.

- **Accepts:** both sides compare the same ordered pair of statics; the
  lexicographically smaller static is the connection initiator for the
  life of the connection, which also fixes stream-ID parity. Exactly one
  connection exists on each side, and it is the *same* connection.
  Holds under both orderings, including when one side reaches the
  decision via the staged path and the other via the internal tie-break.
- **Anchor:** §6.6, §6.7 (ruling 35). **Paused clock:** yes.
- **Note:** bubble-engine currently implements its own "the lower key
  dials" convention at the application layer. If this story holds,
  that convention can be deleted.

### S5 — a connection with no traffic at all is reaped, and the user may hold it open

- **Accepts:** a connection that carries *no* application traffic after
  install emits nothing and dies at install + `DEAD_TIMEOUT` (25 s) with
  `ConnectionLost::TimedOut`. A connection that has carried **one
  exchange in either direction** self-sustains indefinitely via the
  10 s keepalive dance, with no opt-in.
- **Accepts (opt-in):** `set_persistent_keepalive(Some(i))` holds an
  otherwise-idle link open and refreshes a NAT binding. It returns
  `Result<(), ConfigError>`: `Err` below 1 s and `Err` at or above
  `DEAD_TIMEOUT`, and a rejected call leaves the current interval
  **unchanged** rather than clamping.
- **Anchor:** §7.4's install pin, §7.5 (rulings 39, 40, 42, 44).
  **Paused clock:** yes.
- **⚠ CHECK:** an application that connects *ahead of need* and then sits
  silent loses the connection at 25 s. This is ruled and deliberate — the
  application redials when it has something to say — but it is the story
  most likely to surprise a consumer, and it must be prominent in the
  crate docs, not buried.

---

## B. Inbound admission — the staged accept

The ladder is the point: **0 DH to see it, 1 to inspect it, 2 to prove
it, 4 to accept it.** Each stage is a place the application may stop.

### S6 — a user can reject an inbound connection without doing any DH *(maintainer's #3)*

- **Accepts:** an inbound initiation whose mac1 verifies is parked and
  surfaced as an `Intro`. Rejecting it costs **0 DH** — no curve
  operation has run. The peer's static is *not* revealed at this stage;
  the application decides on source address and arrival alone.
- **Anchor:** §6.3, §6.5, §6.1's ladder. **Paused clock:** yes.
- **Test bar:** a counting DH provider asserts `dhs == 0`.

### S7 — a user can inspect an identity for 1 DH, reject it, and denylist it *(maintainer's #4)*

- **Accepts:** `read_identity()` on a parked `Intro` yields the
  **claimed** static and a `Claimed`, costing exactly **1 DH** (`es`).
  Rejecting a `Claimed` costs 1 DH in total — the proving `ss` is never
  paid. The claimed static is not yet *proven*: it is what the initiator
  asserts, sufficient to decide against, not to trust.
- **Accepts (rejecting by identity — CORRECTED, ruling 48):** the
  application may reject **at any stage, for any reason**, including
  consulting a list it owns. **slither provides no ban list and keeps no
  such state**: §6.1 forbids anything durable keyed on the claimed
  static, the source address or `sender_index`, because all three are
  attacker-choosable. Rejecting by identity therefore costs **1 DH**, not
  0 — the static is simply not knowable at the `Intro` stage, so there is
  no reject-by-identity before `read_identity()` spends the `es`.
- **Anchor:** §6.1, §6.2, §6.5, Appendix A.1. **Paused clock:** yes.
- **Test bar:** hiss pins this — `dhs == 1` after intro, and a
  drop-to-reject path costing exactly one DH.
- **⚠ CHECK:** the distinction between *claimed* and *proven* is the
  sharpest edge in the whole API. An application whose list is keyed on a
  **claimed** static bans on an **unauthenticated assertion** — an
  attacker can claim any public key to get a third party banned. Deciding
  on a **proven** static (S9, 2 DH) avoids this entirely. That trade is
  the application's to make; slither's job is to make both stages
  reachable and to keep nothing itself.

### S8 — a user can park a decision across event-loop turns, including for a human

The staged chain *suspends*; it does not merely decide synchronously.

- **Accepts:** a `Claimed` is an owned, app-held object with no lifetime,
  parked across turns while a human is asked, a directory is queried, or
  a policy is fetched. It expires at `INTRO_TTL` (15 s) if not resolved.
  It is `#[must_use]` and not `Clone`, so a parked chain cannot be forked.
- **Anchor:** §6.3, §6.5, Appendix A.1. **Paused clock:** yes.
- **Note:** this is the entire reason the split read exists in hiss, and
  the reason the synchronous `_with` closure was rejected as a fallback.
  A human-in-the-loop pairing ceremony is the motivating case.

### S9 — a user can prove an identity, then still decline

- **Accepts:** `authenticate()` takes `Claimed` → `Proven` at **2 DH
  cumulative** (`ss`), and the static is now *proven*, not merely
  claimed. Declining a `Proven` still installs nothing. `accept()` takes
  it to a live connection at **4 DH** total.
- **Anchor:** §6.1, §6.2. **Paused clock:** yes.

### S10 — a flood of inbound initiations does not disturb established connections

- **Accepts:** the stage-0 queue is capped at 1024 entries with at most 4
  per source and a 15 s TTL. Under saturation, established connections
  keep running; the cost to hold a parked entry is bounded (measured
  mid-state 784 B on P-256, so 1024 parked ≈ 0.77 MiB).
- **Anchor:** §6.3, §6.9, §17.5. **Paused clock:** yes.

### S11 — a user is told when a connection they dialled is contested

The visible half of ruling 36/41.

- **Accepts:** when an accept is refused because the live connection's
  basis is `None` (S3c), that connection is marked contested: an
  ack-eliciting PING goes out and an ACK covering any counter at or above
  the recorded **probe floor** must arrive within `KEEPALIVE_TIMEOUT`. A
  live peer answers and the connection survives, the refusal standing. A
  peer that cannot answer dies with `ConnectionLost::TimedOut` and the
  parked `Intro` becomes acceptable on the next attempt.
- **Accepts:** a second refusal while already contested creates no second
  mark, sends no second PING, and does **not** re-arm the deadline.
- **Accepts (rulings 45 + 46):** the marking is **application-visible**,
  delivered through §16.2's notification stream — not the core's internal
  `ConnEvent`, which never reaches an application. The notification is
  emitted at **probe transmission** (not at marking: the two can separate
  under the amplification budget), and its resolution is observable.
  Without it a consumer cannot distinguish *"refused but healthy, keep
  using it"* from *"refused and about to die, prepare to redial"* — and
  bubble-engine's re-dial scheduler is exactly the code that must tell
  those apart. **No wire change.**
- **Anchor:** §6.4, §7.5, §16.2 (rulings 36, 41, 43, 45, 46).
  **Paused clock:** yes.

---

## C. Data transfer

### S12 — a user can stream *(maintainer's #5)*

- **Accepts:** open a stream, write, the peer reads the same bytes in the
  same order with no gaps or duplicates, `finish()` delivers the FIN, the
  reader observes end-of-stream. Survives loss, reordering and
  duplication on the path.
- **Anchor:** §9, §10, §11, §12, §13. **Paused clock:** yes, over
  `FlakyWire`.

### S13 — a user can run several streams at once without head-of-line blocking

- **Accepts:** concurrent streams are independent; loss on one does not
  stall another. Stream IDs carry the initiator/responder parity fixed at
  S4.
- **Anchor:** §9.1, §10. **Paused clock:** yes.

### S14 — a user can abandon a stream without killing the connection

- **Accepts:** reset a stream; the peer surfaces
  `ReadError::Reset(code)`; other streams and the connection are
  unaffected.
- **Anchor:** §9, §18.1. **Paused clock:** yes.

### S15 — a user can send an unreliable datagram

- **Accepts:** `send_datagram` never waits — it drops oldest under
  pressure rather than blocking — and delivery is unordered and
  unreliable by contract. Oversize input is `DatagramError::TooLarge`,
  not a silent truncation.
- **Anchor:** §11.3, §18.1. **Paused clock:** yes.

### S16 — a user can send a single-shot message

- **Accepts:** a message up to **`MESSAGE_RECV_MAX`** (262 144) in one
  call; larger is `MessageError::TooLarge`. (bubble-engine chunks above
  this and cares about the exact bound.) A datagram's own bound is
  separate: `MAX_DATAGRAM_PAYLOAD` = 1169.
- **Anchor:** **§9.8**, §18.1.

### S17 — a slow reader applies backpressure instead of collapsing the sender

- **Accepts:** flow-control credit bounds unacknowledged data per stream
  and per connection; a reader that stops reading stalls its own stream,
  not the connection, and the sender learns rather than buffering without
  limit.
- **Anchor:** §10, §17.5. **Paused clock:** yes.

---

## D. Mobility

### S18 — a connection survives the peer changing network *(maintainer's #6)*

Home Wi-Fi → 5G, with the connection intact.

- **Accepts:** an authenticated, window-fresh packet from a new address
  re-homes the connection; streams continue with no re-handshake and no
  data loss. The application observes `Notification::AddressMoved` and
  `remote_address()` reflects the new address.
- **Accepts (RESOLVED at approval):** roaming is driven by
  *authenticated receipt*, so **the mover must send — and the keepalive
  is what does it.** A peer whose dance is running (S5) carries its own
  move within `KEEPALIVE_TIMEOUT`. A peer that moves and stays silent is
  indistinguishable from one that vanished and dies at `DEAD_TIMEOUT`.
  This is a **positive obligation on the mover**, not a transport probe.
- **Anchor:** §7.3, §7.2, §7.5. **Paused clock:** yes.
- **Note:** the obligation bites hardest on a *quiet mobile* peer,
  because the dance only runs on a connection that has already carried
  traffic (S5). Such a peer should enable the beacon.

### S19 — a connection survives *our* address changing, and a NAT rebind

- **Accepts:** we change interface or the NAT rebinds our mapping; the
  peer re-homes to our new address on our next authenticated packet.
  With the beacon enabled (S5) the binding is refreshed before the NAT
  drops it.
- **Anchor:** §7.3, §7.5. **Paused clock:** yes.
- **⚠ CHECK:** the amplification/roaming interaction was reviewed and the
  probe is not an address-steering reflector. But an application-visible
  story for "we moved" versus "they moved" may want distinct handling;
  today both surface as address changes.

### S20 — a peer that restarts gets a working connection back

- **Accepts:** the peer loses all state and re-handshakes. Where we were
  the responder, the fresh initiation replaces the zombie immediately
  (S3b). Where we dialled, the refusal plus contested probe (S11)
  resolves it within `DEAD_TIMEOUT`, after which the reconnect succeeds.
- **Anchor:** §6.8, §7.5. **Paused clock:** yes.

---

## E. Identity and crypto

### S21 — a user can hold the private key in a Secure Enclave

The story that forces the architecture.

- **Accepts:** an application supplies a DH provider whose key material
  is **non-exportable** and whose handle is not `Send` — an iOS Secure
  Enclave key, a TPM, a smartcard. The endpoint drives it without
  requiring `Send` anywhere on the actor path.
- **Anchor:** the `!Send` single-actor invariant (CLAUDE.md), §16.
- **Note:** `bubble-engine` already does exactly this
  (`EnclaveSlitherIdentity`). This is why the actor is `!Send`, and any
  change that adds a `Send` bound breaks iOS.

### S22 — a user can pick a crypto suite, and mismatches fail closed

- **Accepts:** the suite is declared once via the macro; a peer on a
  different suite, or a wrong static, fails the handshake and installs
  nothing. An unknown version byte is dropped silently — there is no
  negotiation, ever.
- **Anchor:** §1.1, §2, §3.1; §4, §6.1 (clause 4 — added by ruling 275:
  the wrong-static clause's entire mechanism is mac1, which the original
  anchors stop exactly short of). **Paused clock:** yes.

### S23 — a long-lived connection rekeys itself without the user noticing

- **Accepts:** the per-direction epoch ratchet advances every 65 536
  messages with no handshake, no round trip and no application-visible
  event. There is no DH re-handshake; a new handshake from a live static
  means replacement (S3b), not rekey.
- **Anchor:** §7.7. **Paused clock:** yes.

---

## F. Operational

### S24 — a user can drive the whole protocol without a kernel

- **Accepts:** two endpoints over an in-memory wire on tokio's paused
  clock exercise handshake, streams, loss, roaming and every timer
  (5 s/10 s/15 s/25 s/90 s) in virtual time. This is how *all* of the
  above is tested.
- **Anchor:** §16.10; `testutil::FlakyWire` is named in the spec, while
  `testutil::{Network, FlakyPolicy}` are real in `src/testutil.rs` but
  unattested in spec text — an attestation gap to close, since a
  downstream crate already depends on all three.

### S25 — a user can supply the wire, and a send failure is explicable *(RESCOPED, ruling 49)*

The original story — "a failing send surfaces as itself" — was
**withdrawn** on the maintainer's pushback, and the reasoning is worth
keeping.

- **Accepts:** the `Wire` trait is normative and the application supplies
  it — `send_to`/`recv_from` over `io::Result`, not required to be
  `Send`, with `testutil::FlakyWire` as the in-memory implementation the
  paused-clock tests ride.
- **Accepts:** a failing `send_to` is **traced** against the connection
  (§18.2's operator contract), so a `DEAD_TIMEOUT` death is explicable
  rather than a bare timeout.
- **Deliberately NOT accepted:** a send failure does **not** kill a
  connection and raises no application error. Liveness is
  **receive-driven** by ruling (§7.4), and `ENETUNREACH` on one interface
  is exactly the signal that *precedes* a successful roam (§7.3, S18/S19)
  — acting on it would undo the migration guarantee. The application
  supplies the `Wire`, so it already observes every `io::Error` with its
  destination address; slither's obligation is to make the failure
  **explicable**, not to act on it. §18.1's taxonomy stays closed.
- **Anchor:** §16.2/§16.3 (the `Wire` trait), §18.2, §7.4, §7.3.

### S26 — dropping every handle tears everything down cleanly

- **Accepts:** teardown triggers when **every** handle is dropped — not
  when the `Endpoint` is dropped. A live `Connection` handle alone keeps
  the driver running after the `Endpoint` is gone. Once the last handle
  goes, nothing leaks and no task outlives the `LocalSet`.
- **Anchor:** §15.4, §16.3. **Paused clock:** yes.
- **⚠ CHECK:** this is drop-order sensitive and the opposite of the
  obvious guess ("drop the endpoint, everything stops"). It belongs in
  the docs beside S3a.

---

---

## G. Added by rulings 46–47 *(APPROVED 2026/08/14)*

### S27 — a user can await a connection's death, and be told when it moves or is contested

The gap ruling 46 closed: before it, §16.2 had verbs and four accessors
and **no way to learn anything asynchronously**.

- **Accepts:** `Connection::closed().await` resolves with the
  `ConnectionLost` reason whenever the connection ends — for *any*
  reason, whether or not the application is inside a verb call. This is
  what a re-dial scheduler waits on.
- **Accepts:** a narrow per-connection notification stream delivers the
  application-relevant events — `AddressMoved` (S18/S19) and the
  contested marking (S11). It deliberately does **not** mirror the core's
  internal `ConnEvent` variants: `StreamReadable`, `MessageReadable` and
  the rest are already served by the blocking verbs, and duplicating them
  would give two ways to learn the same thing.
- **Anchor:** §16.2, §16.4 (ruling 46). **Paused clock:** yes.
- **Note:** the shell *translates* core events into these; the sans-io
  core is unchanged. bubble-engine drives an endpoint-level event stream
  today, so this is also the migration surface.

### S28 — a user can wait until what they sent is acknowledged, then close

The "farewell message" fix (ruling 47).

- **Accepts:** the application can await **transport-level
  acknowledgement** of what it has sent, then `close()` without loss.
  Before this, `send_message().await` and `finish()` both resolved
  *before* acknowledgement while §15.2 permitted `close()` to drop
  stream and recovery state immediately — so message-then-close lost the
  last message at the path's loss rate, silently, with no API able to
  prevent it.
- **Does not promise:** that the peer's *application* processed it.
  Acknowledgement is transport receipt, nothing more.
- **Anchor:** §9.8, §15.2, §16.2 (ruling 47). **Paused clock:** yes —
  send, await acknowledgement, close, assert the peer received it, with
  loss injected.

---

## H. Added by rulings 50–52 *(APPROVED 2026/08/14)*

Both were listed as "still open" above until a re-read found them already
ruled. They are acceptance criteria for behaviour that is **already
ratified**, not new design — but nothing was testing them, which is the
hole this section closes.

Both were amended at approval, each gaining one criterion: S29 pins that
a **retry loop replaces rather than accumulates**, and S30 gains
**ruling 59** — the receiver must trace the overflow reset, because it is
the end that caused the conflict and the only end that otherwise learns
nothing.

### S29 — a user can give up on a dial and immediately redial

The idiom every consumer writes, made safe (ruling 50).

- **Accepts:** `timeout(d, endpoint.connect(addr, static))` — or any
  `select!` that drops the `Connecting` — **cancels the attempt**. The
  retransmit train stops, nothing is transmitted, and a `connect()` to
  that same static **on the very next line** succeeds rather than
  returning `AlreadyConnected`. The cancellation is ordered ahead of any
  endpoint verb issued after the drop returns, so the redial cannot
  observe the corpse.
- **Accepts:** an initiation arriving from that peer after a cancelled
  dial takes the ordinary staged-accept path (§5.4's NONE row) — there is
  no stale pending for the tie-break to consult, and the timestamp guard
  is unwritten, exactly as at a `HANDSHAKE_GIVEUP` expiry.
- **Accepts (the retry loop does not accumulate):** dial → cancel →
  redial, repeated, leaves the responder with exactly **one** connection
  per cycle, not a growing set of half-open sessions. Its
  `replacement_basis` is `Some(t)` — it was the responder — and each
  fresh initiation carries a strictly newer timestamp, so each accept is
  a **replacement** (S3b), which one-connection-per-static (§16.1) makes
  the only available outcome. Cost per cycle is bounded: 4 DH and one
  `ConnectionLost::Replaced`. Asserted over N cycles on the paused clock,
  counting live sessions.
- **Does not promise:** that a peer which already answered is told. It
  holds a half-open session and reaps it at `DEAD_TIMEOUT` (25 s) in
  silence — §7.4's install pin means it emits nothing at all.
- **Anchor:** §16.3, §16.1, §5.4, §6.4, §17.1 (ruling 50). **Paused
  clock:** yes — Appendix B pins the immediate-redial ordering.
- **Note:** the property above holds *through* S3b, but it is tested
  here from the direction a consumer actually reaches it — a timeout
  retry loop — rather than from the replacement story.

### S30 — a user who mixes messages with uni streams fails loudly, not silently

The one configuration slither cannot repair, made diagnosable (rulings
51–52).

- **Accepts:** mixing `recv_message()` with `accept_uni()` on one
  connection is a **programming error** by ruling — the wire carries no
  discriminator, so no implementation can repair it. `open_bi()`
  alongside messages is safe and is the documented alternative; the other
  is to tag in band.
- **Accepts:** when the rule *is* broken, the failure is bounded and
  named. An unclaimed uni stream that fills `MESSAGE_RECV_MAX` (262 144)
  while a `recv_message()` claim is pending is reset by the receiver;
  the sender observes `WriteError::Reset(MESSAGE_OVERFLOW)` — code
  `0x06`, distinguishable from a peer's `reset(0)` and from a dropped
  `SendStream` — instead of stalling for ever with keepalives flowing.
- **Accepts (the guard is load-bearing):** a receiver in *stream* mode
  that is merely slow to call `accept_uni()` is **not** reset. That is
  §16.4's backpressure-by-retention working as designed, and the
  unguarded check would break an ordinary lazy accept loop.
- **Accepts (both ends are explicable — ruling 59):** the **sender**
  learns via `WriteError::Reset(MESSAGE_OVERFLOW)`. The **receiver** —
  the end whose verb choice actually caused the conflict — has no error
  and no notification, so it **MUST** trace the reset under §18.2's
  `slither::frames`, naming the stream, its final size, and the mode
  conflict. Symmetric with ruling 49's failing-send obligation: slither
  does not act on it, but it makes it explicable. The concrete
  post-mortem is an operator asking why transfers to this peer die at
  exactly 256 KiB, who reads the **receiver's** log to find out.
- **Anchor:** §9.8, §15.3, §18.1, §18.2 (rulings 51, 52, 59). **Paused
  clock:** yes.
- **⚠ CHECK:** this is the fourth documentation obligation and the
  sharpest of them, because the safe and unsafe shapes look alike at the
  call site. It belongs in rustdoc on `send_message`, `open_uni` **and**
  `accept_uni`, not only in the spec.

---

## I. Composability *(APPROVED 2026/08/16 — ruling 209)*

Drafted in `PLAN.md` §7 and moved here on the maintainer's approval. The
composability layer is new capability, so it owes stories on the same
terms as everything else — and slice 8's planner found it had none,
because its brief cited these three as already living here.

### S31 — a user can treat a stream as an `AsyncRead`/`AsyncWrite`

The ecosystem's byte-stream shape, on the object that actually is one.

- **Accepts:** `tokio::io::copy` a file into a `BiStream` behind a
  `BufWriter`, then `shutdown()`, and the peer reads identical bytes and
  observes EOF. Over `FlakyWire` with loss, on the paused clock.
- **Accepts:** errors arrive as `io::Error`, and a peer's reset surfaces
  as `ConnectionReset` rather than as a silent truncation — the failure
  mode a byte-stream consumer cannot otherwise distinguish from a clean
  end.
- **Accepts:** `poll_shutdown` is `finish()` **and then** `acked()`
  (ruling 57), so `copy(…).await; shutdown().await` does not lose its
  tail. It resolves in error if the connection dies first, so it cannot
  hang past `DEAD_TIMEOUT`.
- **Does not promise:** that `flush()` means delivery. `poll_flush` is a
  no-op returning `Ready` (ruling 56) — bytes accepted by `poll_write`
  are already in send state, and there is no shell buffer to push.
- **Anchor:** §16.11, §3.1 of `PLAN.md`, rulings 55–57. **Paused clock:**
  yes.

### S32 — a user can stream typed objects with a codec

Framing, for free, from the byte streams.

- **Accepts:** `Framed<BiStream, LengthDelimitedCodec>` round-trips a
  sequence of objects **in order**.
- **Accepts:** `Stream`/`Sink` backpressure maps onto flow-control credit
  rather than onto an intermediate buffer.
- **Accepts (the invariant, asserted rather than described):** a consumer
  that polls **once** finds that **exactly one** item was claimed. This
  is §16.11 / ruling 58 — *an adapter never claims ahead of its
  consumer* — and it is the single easiest way to get this layer wrong,
  because a read-ahead task looks like an ergonomic convenience and is
  in fact the unbounded shell queue §10.6 forbids.
- **Anchor:** §16.11, §10.6, ruling 58. **Paused clock:** yes.

### S33 — a user can drive slither from a `tower::Service`

The service shape, typed honestly rather than aspirationally.

- **Accepts:** a `Service` call opens **one bi stream**, writes the
  request, finishes, and reads the response to EOF. The stream *is* the
  request/response correlation, because the wire carries no request id
  and a `Service` over the message verb therefore cannot work.
- **Accepts:** concurrent calls do not head-of-line block each other.
  `serve()` drives the accepting side.
- **Accepts (the caveat is rustdoc, not folklore):** the `!Send`
  boundary is asserted — `UnsyncBoxService` composes; `tower::buffer::Buffer`,
  `spawn_ready`, `BoxService`, hyper and plain `tokio::spawn` do not,
  because they spawn onto a work-stealing executor.
- **Does not promise:** a `Send` façade. D6's `bridge` is deliberately
  out of v0.2 scope — it re-crosses the core→shell seam with channels,
  which is exactly where round 7's five defects lived.
- **Anchor:** §3.4 of `PLAN.md`, §16.3, S21. **Paused clock:** yes.

## J. Liveness of the accept loop *(APPROVED 2026/08/17 — ruling 252)*

The 2026/08/17 audit's C1: one lost msg2 against a one-accept responder
fails the dial outright — measured 1 in 12 at 10 % loss. The design is
confirmed (msg2 is never retransmitted; the peer re-offers fresh
initiations, §5.5); what was missing was any statement of the
application's obligation to keep accepting, and a harness that could
express the case — every `accept()` site in `tests/` accepted exactly
once (working rule 13).

### S34 — a responder that keeps accepting survives a lost msg2

The application obligation §6.5 now states, exercised end to end.

- **Accepts:** drop exactly one msg2 with `FlakyWire`; the initiator's
  same `connect()` resolves within §5.5's retransmit schedule — no new
  dial, no application retry. The responder's accept loop admits the
  peer's fresh initiation; its first, never-confirmed connection
  surfaces `ConnectionLost::Replaced`; the replacement carries the
  traffic.
- **Accepts:** the dial story under loss — at 10 % random loss, 12 of 12
  establishments complete against a looping responder (the measured
  1-in-12 failure of a one-accept responder goes to 0).
- **Cost:** 4 DH per completed `accept()` ladder — the recovery pays the
  ladder twice at the responder, once per admitted initiation (§6.1).
- **Anchor:** §5.5, §6.4, §6.5, ruling 252. **Paused clock:** yes.

---

## Carried forward from the walkthrough

**Resolved at approval:**

- **S11's observability** → contested becomes application-visible
  (**ruling 45**, a new `ConnEvent`, no wire change). To apply.
- **S18's mover obligation** → the mover keepalives. Folded into S18 as
  an acceptance criterion; no transport-side probe is added.
- **S3's shape** → the three-way split is confirmed as correct.

**Documentation obligations** — real hazards with no code change, each
of which a consumer will otherwise meet by getting it wrong:

1. **S3a** — "call `connect()` again" is the natural guess and returns
   `AlreadyConnected`. The docs must lead with `close()`-then-dial.
2. **S7** — denylisting on a *claimed* static bans on an unauthenticated
   assertion; an attacker can claim any public key to get it banned.
   Belongs in rustdoc at `read_identity()`, not only here.
3. **S5** — connect-ahead-of-need loses the connection at 25 s. Ruled and
   deliberate, and the single most surprising behaviour for a new
   consumer.

4. **S26** — teardown triggers on dropping **every handle**, not the
   endpoint. Drop-order sensitive, and the opposite of the obvious guess.

**Resolved by the review pair (rulings 46–49):**

- **S25 withdrawn and rescoped.** A send failure does not kill a
  connection and raises no application error — liveness is
  receive-driven, and `ENETUNREACH` is the signal that *precedes* a
  successful roam. What survives is the `Wire` trait being specified at
  last, plus a trace obligation so a 25 s death is explicable. bubble's
  complaint is answered without making sends authoritative.
- **S7's denylist** belongs to the application; slither keeps nothing.

**Closed since — both were ruled (2026/08/14):**

- **Dropping a `Connecting`** → **ruling 50** (§16.3). Dropping the
  future **cancels the attempt**: the retransmit train stops, the static
  leaves PENDING for NONE, and an immediate redial succeeds instead of
  returning `AlreadyConnected`. The cancellation is ordered ahead of any
  subsequent endpoint verb, so `timeout(5 s, connect(..))` — the
  near-universal idiom, since `HANDSHAKE_GIVEUP` is 90 s and there is no
  configurable connect deadline — is safe rather than a trap with no
  escape. **Owed a story: see S29.**
- **Messages and uni streams sharing one incoming supply** → **rulings
  51 + 52** (§9.8). Mixing the two receive modes on one connection is
  **normatively a programming error**, because the wire carries no
  discriminator and the receiver's verb choice alone decides how a uni
  stream is interpreted. The failure is defined and loud rather than a
  permanent stall: an unclaimed uni stream that fills its initial window
  while a `recv_message()` claim is pending is reset with
  `MESSAGE_OVERFLOW` (`0x06`) — the one receiver-emitted reset — so the
  sender learns *which* hazard it hit. `open_bi()` alongside messages is
  safe; it is `open_uni()` + `send_message()` that collides.
  **Owed a story: see S30.**

**Watch item, not reopened:** S8's `INTRO_TTL` of 15 s bounds a
human-in-the-loop decision. A phone-to-phone glyph confirmation may
plausibly exceed it. The constant is ratified and load-bearing for the
flood bound (`1024 / 15 s ≈ 68 packets/second`), so raising it would
weaken a published DoS figure — recorded here so that if the pairing
ceremony hits it in practice, the trade is already written down.

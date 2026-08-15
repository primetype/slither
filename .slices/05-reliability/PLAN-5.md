# PLAN-5 — Slice 5: Reliability (§12 ACK, §13 loss/RTT/PTO, §14 NewReno)

Planner working notes + binding plan. HEAD at planning time: `d39ed9b`.
Stories closed: **S12** (full, over `FlakyWire` with loss), **S28**.

> Status: complete. Sections §0–§11 below; Appendix R is the raw research record.

## §0. Provenance

Everything asserted here was read at HEAD `d39ed9b`. Spec citations are
`SPEC.md:line`; ruling citations are `.spec-v2-clean-slate/rulings.md:line`;
code citations are `path:line`. Appendix R at the foot of this file is the raw
research record (working rule 2), kept so a reviewer can check a claim without
re-reading the spec. **§12/§13/§14 were read in full** (SPEC.md:3553–3919, 367
lines); §7.2, §7.4's quiet set, §8.3–8.7, §10.6, §15.2, §16.2's `acked()`
paragraphs, §16.4's core block, §16.5 and §17.5 by targeted read. `SPEC.md` was
never read whole.

**Three findings below change what gets built.** They are §6.1 (ruling 128
needs *less* work than its rationale says, and does not touch ruling 81),
§8-H1/§10-Q4 (the congestion window changes what `write()` *means*, which is
the real centre of gravity of this slice), and §9-C3 (three rulings and three
shipped doc comments assign §13/§14 to slice 7; `PLAN.md` assigns them to
slice 5, and two inherited debts move with them).

---

## §1. Cut recommendation

**Recommendation: cut in two — 5a (core reliability) and 5b (shell) — run
sequentially, with a two-author blind split inside 5a. Do NOT cut §12 from
§13/§14.**

### Why a cut at all

The argument is not size. It is that slice 5 contains two problems with
**different acceptance evidence**, and a review boundary is only worth
drawing where the evidence changes shape.

- §12/§13/§14 in the core are **arithmetic over an explicit state machine**.
  Their acceptance criterion is numeric and derivable from the spec text alone:
  *given this send schedule and this ACK, `cwnd` is exactly N and `srtt` is
  exactly T*. That is the strongest possible case for a blind author in this
  project — stronger than 4a's, because the spec states the formulae rather
  than describing behaviour.
- `acked()`, ruling 128's drain and the ruling-124 precedence changes are
  **handle-semantics under concurrency**. Their acceptance criterion is an
  application transcript: *this sequence of awaits on a dying connection
  answers this way*. Ruling 128 was found by exactly that kind of writing and
  by nothing else (rulings.md:3301–3305).

Reviewing those two together means one reviewer holding RFC 9002's arithmetic
and `Poll` precedence in the same pass. Slices 4a/4b already split on this
seam and produced zero implementation defects across both.

### Why NOT to cut §12 from §13/§14

This is the cut that looks natural — ACK is a wire feature, RFC 9002 is an
algorithm — and it is the one to refuse.

§12's sent-packet map, §13's loss walk and §14's admission gate are **one
feedback loop through one call site**: `pump()` records into the map, an ACK
resolves entries, the resolution drives the loss walk, the walk drives the
controller, the controller re-opens the gate, and `pump()` runs again. Cut it
between §12 and §13 and the first half ships a sent-packet map whose only
consumers are in the second half — **precisely ruling 113's defect**
(rulings.md:2607–2614: verbs "sit on a type the contract never surfaces, with
no path from a `Connection`, so nothing in 4a can call them"), which is one of
the five debts this slice exists to pay. Repeating it while paying it would be
a poor look and a real cost.

A §12-only slice is also not independently *green*: a build with ACK and no
loss recovery works on a lossless path and hangs forever on a lossy one, so
S12-full cannot be its exit criterion and the slice would end on a story it
did not close.

### Why 5a and 5b are sequential, not concurrent

5b's `acked()` reads `ConnEvent::StreamFinished { r }`, which **never fires in
slice 4** by construction (`mod.rs:1018–1027`; pinned by
`a_send_half_never_reports_finished_because_slice_four_has_no_acks`,
`tests_streams.rs:3288`). Its integration tests are therefore untestable until
5a lands. Running them concurrently would buy nothing and would put two agents
on `src/core/connection/mod.rs`, which working rule 6 forbids among concurrent
agents. Sequential slices may share paths; **concurrent agents may not**.

### The split inside 5a: two blind authors, one implementer

Working rule 12's lesson — two agents blind to each other disagreed on fact
twice, and each alone would have shipped a clean wrong verdict — argues for
*more* independent readings in the densest text, not fewer. §13 and §14 are
the densest text in the spec by threshold-per-line.

| agent | writes tests for | file it alone owns |
|---|---|---|
| **5a-test-ack** | §12 in full: derivation from the replay window, `MAX_ACK_RANGES`/capacity truncation, `ack_delay`, the delayed-ACK policy and its three triggers, §12.5's processing and its ignore-whole rule, the sent-packet map's contents | `src/core/connection/tests_ack.rs` |
| **5a-test-recovery** | §13.1–13.4 and §14 in full: RTT, loss walk, PTO and backoff, NewReno, recovery period, persistent congestion, `bytes_in_flight`, the admission gate, `app_limited` | `src/core/connection/tests_recovery.rs` |
| **5a-impl** | — | every `src/core/connection/*.rs` implementation file (§3) |

Both authors need the same fixture (`Solo`, `RawPeer`, `Pair`, `drain`), which
today lives inside `tests_streams.rs`. **Extracting it is the integrator's
pre-dispatch job** — see §3.

### Where ruling 128 and `acked()` land

Ruling 128's work is shell **and** core, and `acked()` is shell. Both go
**wholly into 5b**, including ruling 128's two-line core guard change. One
agent owning one behaviour end-to-end beats splitting a four-line change
across a slice boundary, and because 5a and 5b are sequential there is no
rule-6 conflict in 5b touching `src/core/connection/mod.rs` after 5a is done.

### Sizing check

`PLAN.md` estimates slice 5 at ≈3 k lines. Against the tree: §12+§13+§14 core
implementation ≈ 900–1 100 lines across three new modules plus ≈ 250 lines of
edits; the two blind test files ≈ 1 200–1 500 lines together; 5b ≈ 350 lines of
shell plus ≈ 400 lines of tests. That lands at ≈ 3.2–3.6 k, consistent with the
estimate and with 4a's shape (73 blind tests, zero implementation defects).

---

## §2. The binding API contract

This section becomes `CONTRACT-5.md`. **Blind agents build against it, so
every return value is stated for every state.** Slice 4a's author guessed
`read`'s convention wrong (rulings.md:2650–2657) and 4b's contract shipped an
internal contradiction two agents resolved two ways (rulings 124, 128). The
rule applied throughout below: *if a verb can answer more than one way, every
way is enumerated, including the ones that look obvious.*

### 2.1 New module: `src/core/connection/ack.rs` (§12)

```rust
/// §12's ACK state — the scalars that live **alongside** the replay window,
/// never a second received-packet record (§12.2).
pub(crate) struct AckState {
    /// Ack-eliciting packets received since the last ACK we packed. §12.4's
    /// "every 2nd" counter. Reset to 0 when an ACK is packed.
    since_ack: u64,
    /// Arrival instant of the packet bearing the window's **current**
    /// greatest. `None` before the first authenticated, window-fresh receive.
    largest_at: Option<Instant>,
    /// Whether that packet was frame-bearing. §12.3: a keepalive's counter
    /// yields `ack_delay = 0`.
    largest_frame_seen: bool,
    /// An ACK is owed and not yet packed.
    owed: bool,
}

/// What receiving one authenticated, window-fresh packet does to §12.4's
/// policy. Returned so the caller arms or disarms `AckDelay` in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckAction {
    /// Nothing owed, nothing armed.
    None,
    /// An ACK is owed now (2nd ack-eliciting, or out-of-order arrival).
    Now,
    /// Arm `AckDelay` at this instant (`now + MAX_ACK_DELAY`).
    Arm(Instant),
}

impl AckState {
    pub(crate) fn new() -> Self;

    /// Fold in one authenticated, **window-fresh** packet.
    ///
    /// `counter` is the packet's §7.1 counter; `prev_greatest` is the replay
    /// window's greatest **before** this packet was marked; `frame_seen` is
    /// false for §3.4's empty-plaintext keepalive; `ack_eliciting` is
    /// `frame::packet_is_ack_eliciting`.
    ///
    /// **Only window-fresh packets reach here** (§7.2: "no replayed packet
    /// ever moves the endpoint or refreshes liveness") — a duplicate must not
    /// advance §12.4's counter.
    pub(crate) fn on_recv(
        &mut self,
        now: Instant,
        counter: u64,
        prev_greatest: Option<u64>,
        ack_eliciting: bool,
        frame_seen: bool,
    ) -> AckAction;

    /// `true` iff an ACK is owed. Does not clear.
    pub(crate) fn is_owed(&self) -> bool;

    /// §12.4's `AckDelay` firing: the ACK becomes owed now.
    pub(crate) fn on_delay_expired(&mut self);

    /// Called when an ACK has been packed into a packet: clears `owed` and
    /// resets `since_ack`. **The caller disarms `AckDelay`.**
    pub(crate) fn on_ack_packed(&mut self);

    /// §12.3's `ack_delay` field, in **microseconds**, saturating.
    /// `0` when the largest was not frame-seen, and `0` before any receive.
    pub(crate) fn ack_delay_us(&self, now: Instant) -> u64;
}

/// §12.2's derivation, newest-first descending, truncated at
/// `MAX_ACK_RANGES` **pairs** or at `room` plaintext bytes, whichever binds.
///
/// Returns `None` iff the window has no greatest (nothing received yet) — in
/// which case no ACK can be owed either.
///
/// `room` is `Packing::room()` at the moment of the call. The returned frame
/// always encodes to `<= room` bytes; if even the first block does not fit,
/// this returns `None` and the ACK **stays owed** for the next packet.
pub(crate) fn derive(
    window: &session::ReplayWindow,
    ack_delay_us: u64,
    room: usize,
) -> Option<frame::Ack>;
```

**Stated scopes, because each has a wrong plausible reading:**

- `on_recv` is called **once per window-fresh packet**, from
  `handle_datagram`, **before** the frame stream is applied. `prev_greatest`
  must be captured before the window marks the counter, or §12.4's
  out-of-order test is always false.
- §12.4's out-of-order test is `counter != prev_greatest + 1`, and
  `prev_greatest == None` ⟹ **immediate** (SPEC.md:3616–3618 says so in terms
  and calls the result harmless).
- A packet that is window-fresh but **not** ack-eliciting neither advances
  `since_ack` nor arms `AckDelay`, and does not make an ACK owed. It *does*
  update `largest_at`/`largest_frame_seen` if it is a new greatest, because
  §12.3's `ack_delay` is measured from the packet bearing the window's
  largest — which may be a keepalive, which is why the `frame_seen` flag
  exists at all.
- `derive` reads only the window. It does **not** consult `AckState`; the
  delay is passed in. This keeps §12.2's "reuse, don't duplicate" honest and
  makes the derivation a pure function a blind author can test in isolation.

### 2.2 New module: `src/core/connection/recovery.rs` (§13)

```rust
/// One ack-eliciting packet in flight. §13.5's record, exactly.
#[derive(Debug, Clone)]
pub(crate) struct SentPacket {
    /// §7.1's counter — slither's packet number. There is one space (§7.8).
    pub(crate) counter: u64,
    pub(crate) time_sent: Instant,
    /// **The full datagram length**: `DATA_HEADER_LEN + ciphertext + tag`,
    /// i.e. `Transmit::data.len()`. Feeds `bytes_in_flight` (§14.5).
    pub(crate) size: u64,
    /// §14.5's flag, recorded at send by us and read by `on_ack`.
    pub(crate) app_limited: bool,
    /// §13.5's "frame identities aboard". **May be empty**: a bare-PING PTO
    /// probe is ack-eliciting (§8.3) and so is tracked (§13.5: only
    /// *non*-ack-eliciting packets are never inserted), but carries nothing
    /// that re-queues.
    pub(crate) frames: Vec<SentFrame>,
}

/// §8.7's three classes, as the identities the map holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SentFrame {
    /// §8.7 `ranges`. `fin` is **ruling 113's flag, carried not inferred**.
    Stream { r: StreamRef, range: Range<u64>, fin: bool },
    /// §8.7 `regenerate` — the identity only; the value is re-read at
    /// retransmission time.
    ResetStream { r: StreamRef },
    MaxData,
    MaxStreamData { r: StreamRef },
    MaxStreams { dir: Dir },
}

pub(crate) struct Recovery {
    sent: BTreeMap<u64, SentPacket>,
    rtt: RttEstimator,
    largest_acked: Option<u64>,
    /// §13.2's `Loss` deadline. `None` when no survivor is inside the
    /// threshold.
    loss_time: Option<Instant>,
    /// §13.3's anchor: the last ack-eliciting **send**.
    last_ack_eliciting: Option<Instant>,
    pto_count: u32,
    /// Maintained incrementally; `debug_assert`ed equal to the map sum.
    bytes_in_flight: u64,
}

/// What one ACK's processing produced, handed to the caller so the caller —
/// not the recovery module — touches streams and the controller.
#[derive(Debug, Default)]
pub(crate) struct AckOutcome {
    /// Frame identities on newly acknowledged packets, in ascending counter
    /// order.
    pub(crate) acked: Vec<SentFrame>,
    /// Frame identities on newly declared-lost packets, ascending.
    pub(crate) lost: Vec<SentFrame>,
    /// One entry per newly acknowledged packet, for §14's `on_ack`.
    pub(crate) ack_events: Vec<(Instant /*sent*/, u64 /*bytes*/, bool /*app_limited*/)>,
    /// `Some` iff a loss episode occurred: §14.3's **once per episode**
    /// congestion event. `sent_time` is the **earliest** lost packet's.
    pub(crate) congestion: Option<CongestionEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CongestionEvent {
    pub(crate) sent_time: Instant,
    pub(crate) is_persistent: bool,
    pub(crate) lost_bytes: u64,
}

impl Recovery {
    pub(crate) fn new() -> Self;

    /// Record one **ack-eliciting** packet. Asserts `!frames.is_empty() ||
    /// probe`, arms nothing — the caller re-reads `deadline()`.
    pub(crate) fn on_sent(&mut self, packet: SentPacket);

    /// §12.5's processing. `highest_sealed` is `next_counter() - 1`.
    ///
    /// Returns `AckOutcome::default()` — a total no-op — when
    /// `ack.largest > highest_sealed` (§12.5's **ignore whole**, with a
    /// trace), and when nothing in the ACK is newly acknowledged.
    pub(crate) fn on_ack(
        &mut self,
        now: Instant,
        ack: &frame::Ack,
        highest_sealed: u64,
    ) -> AckOutcome;

    /// §13.2's timer firing: the same walk with no new acknowledgement.
    pub(crate) fn on_loss_timeout(&mut self, now: Instant) -> AckOutcome;

    /// §13.3's timer firing. Increments `pto_count` (saturating at the
    /// exponent whose multiplier is `PTO_BACKOFF_CAP`) and returns nothing:
    /// the **caller** builds the probe from §13.4's rule.
    pub(crate) fn on_pto_timeout(&mut self);

    /// The `Loss` deadline, or `None`.
    pub(crate) fn loss_deadline(&self) -> Option<Instant>;
    /// The `Pto` deadline. `None` iff the sent map is empty (§13.3's
    /// precondition) or no ack-eliciting send has happened.
    pub(crate) fn pto_deadline(&self) -> Option<Instant>;

    pub(crate) fn bytes_in_flight(&self) -> u64;
    pub(crate) fn is_empty(&self) -> bool;
    pub(crate) fn rtt(&self) -> &RttEstimator;

    /// §13.6's roam seam. Keeps the map; re-seeds `min_rtt` on the next
    /// sample. **Uncalled until slice 7** — see §6/§10-Q9.
    pub(crate) fn on_roam(&mut self, now: Instant);
}

pub(crate) struct RttEstimator { /* latest, smoothed: Option<_>, rttvar, min_rtt */ }

impl RttEstimator {
    pub(crate) fn new() -> Self;
    /// §13.1. `ack_delay` is the peer's raw report; the cap and the `min_rtt`
    /// guard are applied here.
    pub(crate) fn sample(&mut self, latest: Duration, ack_delay: Duration);
    /// `K_INITIAL_RTT` before any sample.
    pub(crate) fn smoothed_rtt(&self) -> Duration;
    /// `K_INITIAL_RTT / 2` before any sample.
    pub(crate) fn rttvar(&self) -> Duration;
    /// §14.4's precondition: "a prior RTT sample exists".
    pub(crate) fn has_sample(&self) -> bool;
    /// §13.2's `max(9/8 * max(srtt, latest), K_GRANULARITY)`.
    pub(crate) fn loss_delay(&self) -> Duration;
    /// §13.3's formula **with `pto_count = 0`** — §14.4 uses exactly this.
    pub(crate) fn pto_interval(&self) -> Duration;
    /// §13.1's roam clause: `min_rtt` MUST be allowed to rise.
    pub(crate) fn reseed_min_rtt(&mut self);
}
```

### 2.3 New module: `src/core/connection/congestion.rs` (§14)

```rust
/// §14.1's seam, **verbatim from the spec** but `pub(crate)`: CUBIC and BBR
/// are §19's, so nothing outside the crate may implement it in v1.
pub(crate) trait Controller {
    fn on_sent(&mut self, now: Instant, bytes: u64);
    fn on_ack(&mut self, now: Instant, sent_time: Instant, bytes: u64, app_limited: bool);
    fn on_congestion_event(
        &mut self,
        now: Instant,
        sent_time: Instant,
        is_persistent: bool,
        lost_bytes: u64,
    );
    fn window(&self) -> u64;
}

pub(crate) struct NewReno {
    cwnd: u64,
    ssthresh: u64,
    /// §14.3's recovery-period marker. `None` = no recovery period has ever
    /// started, so nothing is fenced.
    recovery_start: Option<Instant>,
    /// §14.2's integer appropriate-byte-counting accumulator.
    acked_accum: u64,
}

impl NewReno {
    pub(crate) fn new() -> Self;   // cwnd = INITIAL_WINDOW, ssthresh = u64::MAX
    /// §14.6's roam reset. **Uncalled until slice 7.**
    pub(crate) fn reset(&mut self, now: Instant);
    #[cfg(test)] pub(crate) fn ssthresh(&self) -> u64;
    #[cfg(test)] pub(crate) fn recovery_start(&self) -> Option<Instant>;
}
```

### 2.4 Changes to `core::Connection`

```rust
// CHANGED — ruling 113's known-wrong signature. `fin` is carried off the
// sent-packet map; `frame_carried_fin` (mod.rs:544) is DELETED.
pub(crate) fn on_ack_range(&mut self, now: Instant, r: StreamRef,
                           range: Range<u64>, fin: bool);
pub(crate) fn on_lost_range(&mut self, now: Instant, r: StreamRef,
                            range: Range<u64>, fin: bool);

// NEW — ruling 47's core half.
/// §16.2's snapshot: every byte handed to the connection at this instant.
pub(crate) fn ack_snapshot(&self) -> AckSnapshot;
/// Whether every byte in `snap` is acknowledged **or abandoned by a reset**.
pub(crate) fn snapshot_settled(&self, snap: &AckSnapshot) -> bool;

// NEW — accessors the tests and the shell need.
#[cfg(test)] pub(crate) fn bytes_in_flight(&self) -> u64;
#[cfg(test)] pub(crate) fn congestion_window(&self) -> u64;
#[cfg(test)] pub(crate) fn smoothed_rtt(&self) -> Duration;

/// §16.2's snapshot, opaque. Empty iff nothing had been written.
#[derive(Debug, Clone, Default)]
pub(crate) struct AckSnapshot(Vec<(StreamRef, u64)>);
```

**`snapshot_settled` — every case, because getting one wrong hangs `acked()`
forever:**

| state of a `(r, offset)` entry | settled? | why |
|---|---|---|
| `r` absent from the stream table | **yes** | the half was freed, which happens only at `DataRecvd` or `ResetRecvd` — acknowledged or abandoned |
| send half present, `acked` covers `0..offset` | yes | §16.2 |
| send half present, `reset` is `Some(_)` | **yes** | §16.2: "or abandoned by a reset (§9.6: an abandoned byte is never acknowledged, and waiting on one would never terminate)" |
| send half present, gap in `acked` below `offset` | no | — |
| `offset == 0` (stream opened, never written) | **yes** | vacuous; nothing was handed to the connection |
| snapshot taken, then the stream is written further | **yes**, at the snapshot offset | §16.2: "Bytes written after the call do not extend it" |

**FIN is deliberately NOT part of `Connection::acked()`'s snapshot.** §16.2
scopes it to *"every byte handed to the connection"*, and a connection-level
`acked()` that also waited for a FIN would never terminate on a stream the
application intends to keep open — the exact hazard §16.2 closes with "bytes
written after the call do not extend it". `SendStream::acked()` is the verb
that includes the FIN, and §16.2 says so explicitly (SPEC.md:4369–4372).

### 2.5 Shell — `src/shell/stream.rs`

```rust
impl<S: Handshake> SendStream<S> {
    /// §16.2 / ruling 47. Resolves once every byte written to **this** stream
    /// and its FIN are acknowledged by the peer's transport.
    pub async fn acked(&mut self) -> Result<(), WriteError>;
    pub(crate) fn poll_acked(&mut self, cx: &mut Context<'_>)
        -> Poll<Result<(), WriteError>>;
}
```

**Every outcome, in ruling 124's precedence order:**

1. `local_end == LocalEnd::Reset`, or the stream was reset by the peer's §9.8
   overflow reset → `Err(WriteError::Reset(code))`. §16.2:4373.
2. This half already reached `DataRecvd` (all bytes **and** the FIN acked) →
   `Ok(())`. **This outranks the death latch** — ruling 124's principle: a
   stream that completed did not un-complete when the connection died, and
   reporting `ConnectionLost` over a fully-acknowledged transfer is ruling
   121's misreport with its sign flipped.
3. Connection death latch → `Err(WriteError::ConnectionLost(l))`.
4. Otherwise park in a **new** waker map, `blocked_ackers: BTreeMap<StreamRef,
   Wakers>`, woken by `ConnEvent::StreamFinished { r }` and by
   `ConnEvent::StreamReset { r, .. }`.

**Never `Err(WriteError::Finished)`** — §16.2:4372 in terms: *"It is legal and
expected after `finish()`, and never returns `WriteError::Finished` for that
reason."*

**Before `finish()` it parks and does not resolve on a live connection.** §16.2
requires "every byte … **and its FIN**", and a FIN that was never queued cannot
be acknowledged. This is stated because it is the one shape a caller will write
by accident (`write(..).await; acked().await;`) and it looks like a hang. It
goes in rustdoc at the call site, next to §16.2's own "legal and expected after
`finish()`".

```rust
impl<S: Handshake> Connection<S> {
    /// §16.2 / rulings 47, 54. Snapshot at the instant of the call.
    pub async fn acked(&self) -> Result<(), ConnectionLost>;
}
```

Because the snapshot is taken **at the call** and not at the first poll, this
is written as an `async fn` that takes the snapshot in its body and then
`poll_fn`s over `poll_acked(cx, &snapshot, key)`. Outcomes:

1. `snapshot_settled(&snap)` → `Ok(())`. **Outranks the death latch**, for
   ruling 124's reason and because the alternative reintroduces ruling 128's
   defect on the *sender's* side — see §10-Q1.
2. Connection death latch → `Err(ConnectionLost(l))`.
3. Otherwise park in `closed_wakers`' sibling: a new `settled_wakers: Wakers`
   on `ConnCell`, woken on **every** `StreamFinished` and `StreamReset`, and
   on the latch.

An **empty** snapshot (nothing ever written) resolves `Ok(())` on the first
poll, on a live *or* dead connection.

### 2.6 Shell — ruling 128's post-death drain

```rust
// src/core/connection/mod.rs — CHANGED
pub(crate) fn read(&mut self, now: Instant, r: StreamRef, buf: &mut [u8])
    -> Result<Option<usize>, ReadError>;
```
The unconditional `self.lost` check at `mod.rs:467–469` is **replaced** by:
serve the stream normally if the half is still in the table; return
`Err(ReadError::ConnectionLost(l))` only when `self.lost.is_some()` **and** the
half is gone or has nothing left (no buffered bytes and no pinned FIN reached).

`core::Connection::accept(dir)` needs **no change**: it already has no `lost`
guard (`mod.rs:405–408`) and already hands over unclaimed streams after death.

Shell-side, the ruling-124 precedence gains one step for `read` and `accept_*`
**only**:

| verb | new order |
|---|---|
| `RecvStream::poll_read` | (1) own terminal state → (2) empty-buffer short-circuit → (3) **the core call** → (4) death latch, **only if the core had nothing** → never `Pending` when the latch is set |
| `Connection::poll_accept_{bi,uni}` | (1) **the core call** (`accept(dir)`) → (2) death latch if `None` → never `Pending` when the latch is set |
| every other 4b verb | **unchanged** — ruling 124 stands as ratified |

**"Parking is never permitted on a dead connection"** (rulings.md:3221–3223) is
the load-bearing clause: when the latch is set, a core answer of `Ok(Some(0))`
("no data available") must be converted to `Err(ConnectionLost)`, not to
`Pending`. Getting this wrong hangs a reader forever, which is the same trap
class the `Ok(Some(0))`/`Ok(None)` convention closes.

### 2.7 What slice 5 must NOT build

- **No roaming.** §13.6 and §14.6 are implemented as the two `on_roam` /
  `reset` methods above and are **uncalled**; slice 7 calls them. See §10-Q9
  for the one schema decision that cannot be deferred with them.
- **No pacing, no ECN, no CUBIC/BBR** (§14.7).
- **No datagram tracking** (§11 is slice 6). §13.5's "DATAGRAM markers" get no
  `SentFrame` variant: a datagram's class is `never` (§8.7), so it needs no
  identity — only the packet's `size`, which it already contributes. Recorded
  as slice 6's, and the reason is stated so slice 6 does not re-open the map's
  schema by reflex.
- **No new error variants.** §18.1 is closed by process; `error.rs:3–41`. Both
  `acked()` verbs use existing variants, which §16.2:4412–4416 states as a
  requirement.

---

## §3. Module map and file ownership

`PLAN.md`'s module map already names three files that do not yet exist:
`ack.rs` (§12), `recovery.rs` (§13), `congestion.rs` (§14). Slice 5 creates
exactly those three.

### 5a — three concurrent agents, disjoint paths (working rule 6)

| path | owner | new? |
|---|---|---|
| `src/core/connection/ack.rs` | **5a-impl** | new |
| `src/core/connection/recovery.rs` | **5a-impl** | new |
| `src/core/connection/congestion.rs` | **5a-impl** | new |
| `src/core/connection/mod.rs` | **5a-impl** | edit |
| `src/core/connection/session.rs` | **5a-impl** | edit (`seal_inner`'s commit block, `session.rs:487–491`) |
| `src/core/connection/frame.rs` | **5a-impl** | edit (`Packing::ack` / `::ping` wiring) |
| `src/core/connection/streams.rs` | **5a-impl** | edit |
| `src/core/connection/send.rs` | **5a-impl** | edit |
| `src/core/connection/flow.rs` | **5a-impl** | edit (`send_room`'s second bound) |
| `src/core/connection/timers.rs` | **5a-impl** | edit if needed |
| `src/core/connection/tests_ack.rs` | **5a-test-ack** | new — **this agent's alone** |
| `src/core/connection/tests_recovery.rs` | **5a-test-recovery** | new — **this agent's alone** |

`5a-impl` declares `#[cfg(test)] mod tests_ack;` and `#[cfg(test)] mod
tests_recovery;` in `mod.rs` and **creates neither file** — CLAUDE.md working
rule 6, which exists because a placeholder stub destroyed 68 tests in slice 2a.

### The integrator's files — working rule 15

Three of these must be **done and committed before the blind agents' worktrees
are cut** (working rule 14: an isolated agent sees a commit, not a working
tree; `CONTRACT-4a.md` was uncommitted when 4a's author was cut and it cost ten
minutes and two semantic guesses).

1. **`src/core/connection/testfix.rs` — new, extracted, committed first.**
   `Solo`, `RawPeer`, `Pair`, `Drained`, `drain`, `handshake_pair`, `raw_id`,
   `parse_frames`, `t0` currently live inside `tests_streams.rs`'s private
   `mod tests` (`tests_streams.rs:379–3525`). Both blind authors need them and
   neither may touch `tests_streams.rs`. Extract to
   `#[cfg(test)] pub(super) mod testfix;`, leave `tests_streams.rs` importing
   from it, verify `cargo test` is unchanged, commit. **Name the commit in
   both briefs.**
2. **`src/core/connection/tests_streams.rs` — the `fin` argument.** Ruling 113's
   signature change touches ~24 existing call sites of
   `on_ack_range`/`on_lost_range` (`send.rs`, `streams.rs`, `mod.rs` and their
   test modules). Mechanical, but it is an existing test author's file: the
   integrator does it, **before dispatch**, and commits. Working rule 10:
   commit before mutating.
3. **`Cargo.toml`'s `[[test]]` stanza** for any new integration target
   (ruling 126). Cargo *refuses to parse the manifest* when a `[[test]]` names
   a missing file, which reds every gate at once. The implementer may commit
   it **commented out**; the integrator uncomments it.
4. **`tests/spec_streams.rs:690`** — deleting the `#[ignore]` on
   `a_receiver_can_drain_a_stream_the_sender_closed_behind`. Existing file,
   one line, integrator's, at the end of 5b.
5. **`src/core/connection/tests_streams.rs:3234`** — deleting the `#[ignore]`
   on `credit_frames_precede_the_stream_fill_in_a_packet`. **See §6.5: this is
   slice 5's, not slice 7's, and the brief is wrong about it.**

### 5b — two agents, sequential after 5a

| path | owner |
|---|---|
| `src/shell/stream.rs`, `src/shell/connection.rs`, `src/shell/driver.rs`, `src/shell/shared.rs` | **5b-impl** |
| `src/core/connection/mod.rs` (the `read` guard only) | **5b-impl** |
| `tests/story_reliability.rs` — **new**, S12-full + S28 | **5b-test** (blind) |
| `Cargo.toml` `[[test]]` stanza for it | integrator |
| `tests/spec_streams.rs` (un-ignore) | integrator |

`tests/*.rs` reach only the **public** API plus `slither::testutil` behind
`test-util`. `Solo` is a private in-crate fixture and is unreachable from
there, which is why the §13/§14 arithmetic tests must be in-crate and the
story tests must not try to assert `cwnd`.

---

## §4. The algorithms, stated precisely

Every formula below is §13/§14's, with the arithmetic form v0.1 already
validated (`git show 5324ce5:src/recovery.rs`) where one exists. **The scope
rule from the brief is applied: §13/§14 is the sole authority on behaviour; v0.1
is consulted only as a worked reference for the formulae.** One divergence found
and reported at §9-C-v01.

### 4.1 RTT estimation — §13.1. State: `RttEstimator`

```
latest_rtt = ack_arrival − sent_time(largest_newly_acked)      [see §8-H4]

first sample:  smoothed = latest ; rttvar = latest / 2 ; min_rtt = latest
later samples: min_rtt  = min(min_rtt, latest)
               capped   = min(peer_ack_delay, MAX_ACK_DELAY)
               adjusted = if latest >= min_rtt + capped { latest − capped }
                          else { latest }
               rttvar   = rttvar * 3/4 + |smoothed − adjusted| / 4
               smoothed = smoothed * 7/8 + adjusted / 8

before any sample: smoothed_rtt() = K_INITIAL_RTT (333 ms)
                   rttvar()       = K_INITIAL_RTT / 2
```
A sample is taken iff **`ack.largest` is newly acknowledged**. §13.1's second
condition ("at least one newly acknowledged packet is ack-eliciting") is
**vacuous here** and must not be implemented as a separate test — see §8-H2.

Integer arithmetic throughout (`Duration * 3 / 4`), never floats. v0.1's
`RttEstimator::sample` is correct against §13.1 line for line and is the
recommended starting point.

### 4.2 Ack-based loss detection — §13.2. State: `Recovery.{sent, largest_acked, loss_time}`

```
loss_delay = max(9/8 * max(smoothed_rtt, latest_rtt), K_GRANULARITY)

for each entry pn in sent, with pn <= largest_acked:      [see §9-C1]
    by_count = (largest_acked − pn) >= K_PACKET_THRESHOLD   (3)
    by_time  = (now − time_sent) > loss_delay               [see §10-Q12]
    if by_count || by_time  -> LOST
    else                    -> survivor: loss_time = min(loss_time, time_sent + loss_delay)

entries with pn > largest_acked are not judged at all.
```
`loss_time` is **recomputed from scratch** on every walk (set to `None` first),
exactly as v0.1 does — a stale `loss_time` arms a timer for a packet already
resolved. Lost entries leave the map; their `size` leaves `bytes_in_flight`;
their `frames` re-queue by §8.7 class.

### 4.3 Probe timeout — §13.3. State: `Recovery.{last_ack_eliciting, pto_count}`

```
pto_base = smoothed_rtt + max(4 * rttvar, K_GRANULARITY) + MAX_ACK_DELAY
deadline = last_ack_eliciting + pto_base * min(2^pto_count, PTO_BACKOFF_CAP)

armed iff !sent.is_empty()                    (§13.3, RFC 9002 §6.2.1)
Loss takes precedence: `Timers::due` already suppresses Pto when Loss is due
  (timers.rs:190–192) and `take_due` disarms after the collapse, so a
  suppressed Pto stays armed. Nothing to add.
pto_count = 0 whenever any packet is newly acknowledged.
pto_count += 1 when the Pto timer fires.       [see §10-Q5]
```

**`PTO_BACKOFF_CAP` is 64 — the multiplier, not the exponent**
(`constants.rs:368–369`, SPEC.md:6287's table: *"written '2⁶'"*). **v0.1 writes
`1u32 << self.pto_count.min(PTO_BACKOFF_CAP)` with its own `PTO_BACKOFF_CAP =
6`.** Copying that idiom with slither's constant shifts by up to 64 and is
undefined behaviour on `u32`. This is the single most likely mechanical error
in the whole slice and it belongs in the contract as a named trap.

### 4.4 Probe content — §13.4

One ack-eliciting packet per firing: pending retransmittable frames
**oldest-first** if any exist, else a **bare PING**. Sealed `seal_quiet` (§7.4's
quiet set names PTO probes explicitly) **and** exempt from the admission gate —
§13.4 calls these "two independent properties of the same send, for different
reasons". Still tracked in the sent map and still counted in
`bytes_in_flight` (§14.5's ruling-43 bullet, §17.5's caveat). Not exempt from
§7.3's amplification budget — which does not exist until slice 7, so slice 5
implements the cwnd exemption only and records the budget as slice 7's.

### 4.5 NewReno — §14.2. State: `NewReno.{cwnd, ssthresh, acked_accum}`

```
init: cwnd = INITIAL_WINDOW (12 000), ssthresh = u64::MAX, acked_accum = 0

on_ack(now, sent_time, bytes, app_limited):
    if app_limited                      -> return          (§14.5)
    if in_recovery(sent_time)           -> return          (§14.3, RFC 9002 §7.3.2)
    if cwnd < ssthresh:  cwnd += bytes                     (slow start)
    else:                acked_accum += bytes              (congestion avoidance)
                         while acked_accum >= cwnd { acked_accum −= cwnd;
                                                     cwnd += MAX_DATAGRAM }

in_recovery(sent_time) := recovery_start.is_some_and(|s| sent_time <= s)
```
`MAX_DATAGRAM` is 1200. The `while` (rather than `if`) is deliberate: a single
large ACK burst may cross the accumulator more than once, and an `if` silently
under-grows.

### 4.6 The recovery period — §14.3

```
on_congestion_event(now, sent_time, is_persistent, lost_bytes):
    if in_recovery(sent_time) { return }        // one cut per episode
    cwnd            = max(cwnd / 2, MINIMUM_WINDOW)     // integer halve
    ssthresh        = cwnd
    recovery_start  = Some(now)
    if is_persistent { cwnd = MINIMUM_WINDOW }          // §14.4
```
`LOSS_REDUCTION_FACTOR` is an `f64` in `constants.rs:392` *"to match §14.2's
notation, not to be multiplied by"* — the code halves with an integer shift.
Fired **once per episode, after the full lost-packet scan** (§14.3's last
sentence), never once per lost packet: `AckOutcome.congestion` is an `Option`,
not a `Vec`, and that is the structural enforcement.

### 4.7 Persistent congestion — §14.4, computed inside §13.2's walk

```
persistent_period = pto_interval_with_pto_count_zero * PERSISTENT_CONGESTION_THRESHOLD (3)

over the LOST set in ascending counter order, and only if rtt.has_sample():
  find the longest run of consecutive lost packets with **no packet
  acknowledged between them**; if (last.time_sent − first.time_sent)
  > persistent_period, is_persistent = true
```
"No packet acknowledged between them" is checked against the acknowledgements
**this walk** produced and against the map: a run is broken by any counter
between `first` and `last` that was acknowledged rather than lost. The
`has_sample()` guard is §14.4's own — *"the pre-sample `K_INITIAL_RTT` phase
never triggers it"* — and without it a first-flight blackhole collapses the
window on the initial-RTT guess.

### 4.8 `bytes_in_flight` and the admission gate — §14.5

```
bytes_in_flight = Σ sent[*].size          (ack-eliciting packets only)
send permitted iff bytes_in_flight + candidate_size <= cwnd
```
Note `<=`, not `<`. Exemptions, **exhaustively** (§14.5 says so): PTO probes;
the contested probe (slice 7); non-ack-eliciting control packets — pure ACKs,
CLOSE, keepalives — which are never tracked and never gated. `candidate_size`
is the full datagram length, so it is computable before the seal as
`DATA_HEADER_LEN + plaintext.len() + AEAD_TAG_LEN` — see §10-Q3.

The gate is evaluated in `pump()`, **after** the plaintext is packed and
**before** the seal, and a packet that does not fit is not sealed and its
frames stay pending. This is the "second bound" `flow.rs:230`'s `send_room()`
doc anticipates, but it does **not** belong in `send_room()`: flow control is a
byte ledger per stream and connection, the gate is a per-packet datagram-size
test, and folding a packet-level bound into a byte-level one is how a build
ends up refusing bytes the peer's credit admits — see §8-H1.

### 4.9 `app_limited` — §14.5

Set when the sender **runs out of queued data with cwnd headroom remaining**,
and **recorded onto each sent packet**. Concretely, in `pump()`: after packing a
packet, `app_limited = !streams.has_output() && bytes_in_flight + size < cwnd`
— i.e. we stopped because we had nothing more to send, not because the window
closed. Recorded on that packet's `SentPacket`. See §10-Q7 for the one
ambiguity.

---

## §5. The sent-packet map

### What it holds
§13.5, verbatim: per ack-eliciting counter — **send time**, **the frame
identities aboard** (stream ranges, credit frame identities, RESET_STREAM,
PING, DATAGRAM markers), and the packet's **size in bytes**. Slice 5 adds
`app_limited` (§14.5 requires it "recorded onto each sent packet") and the
`counter` itself as the key.

### Where it is populated — and the one hard seam
`Session::seal` returns `Sealed { counter, datagram }` (`session.rs:330–337`),
but `pump()` (`mod.rs:906–909`) and `transmit_close` (`mod.rs:792–795`)
**discard `sealed.counter`** and push only `sealed.datagram`. `Transmit` has no
counter field (`core/mod.rs:164–170`).

**The map must therefore be written inside `pump()`, between the seal and the
`Transmit` push, and nowhere else.** `Transmit` must NOT gain a counter field:
it is a public type, it is what the shell hands to the `Wire`, and a packet
number on it would be state the shell could contradict. `seal_inner`'s commit
block (`session.rs:487–491`) already carries a comment naming this insertion
point for *"the dequeue, the pending-ACK clear, `on_sent` and the recovery
timers"* — but `Recovery` lives above `Session`, so the record is assembled in
`pump()` from the `Packing` it just built plus the returned counter. The
`Packing` must therefore be interrogable for its frame identities before
`into_plaintext()` consumes it; `Packing::frames()` (`frame.rs:817`) already
provides it.

### What bounds its size
§17.5, exactly: *"a sent map bounded by cwnd **plus the §14.5 admission
exemptions in flight** (the one-packet PTO probe of §13.4 and, at most, one
contested-connection probe — each ≤ `MAX_DATAGRAM`, so the overshoot is ≤ 2 400
B and never grows with the attack)"*. In slice 5 the contested probe does not
exist, so the bound is `cwnd + MAX_DATAGRAM`. §17.5's caveat paragraph states
the reasoning explicitly and it is worth quoting into the contract: the
exemptions are from **admission**, never from **accounting**, and *"had the
exempt probes been left untracked, the sent map would have been bounded by cwnd
only in the sense that it did not contain the packets it was missing."*

`cwnd` itself has no upper bound in the spec, so the map's bound is
application-governed via what the peer will acknowledge. That is §17.5's
"established connections — application-governed" row and needs nothing new.

### When entries are removed
Three ways, and no fourth:
1. **acknowledged** — §12.5's intersecting processing removes the entry, its
   `size` leaves `bytes_in_flight`, its `frames` become `AckOutcome.acked`.
2. **declared lost** — §13.2's walk removes the entry, its `size` leaves
   `bytes_in_flight`, its `frames` become `AckOutcome.lost`.
3. **state drop** — `Connection::drop_state` (`mod.rs:816`) must clear the map,
   the estimator and the controller. §15.2: *"all stream, flow-control,
   recovery and congestion state may drop immediately"*. `drop_state` currently
   frees `session`, `timers` and `scratch` only; §9-C6 records the conflict
   about what else it should be freeing.

Entries are **never** removed by timeout, by size pressure, or by the packet
being retransmitted — §13.5's "frames, never packets" means a retransmission
creates a *new* entry under a *new* counter while the old one waits to be
acknowledged or declared lost.

### §12.5's bounded intersecting processing
The ACK is intersected against `sent`'s keys, never materialised. With `sent`
in a `BTreeMap<u64, _>` and the ACK's ranges descending, the walk is one
merge pass: `ack.ranges_desc()` (already implemented, `frame.rs:383`) yields
`RangeInclusive<u64>` newest-first, and `sent.range(..)` is queried per block.
Cost is O(in-flight + range_count·log n), inside §12.5's stated
O(in-flight × range_count) bound. **A build that expands the ranges into a
counter set is the failure §12.5 names**, and it is worth a test: a wire-legal
ACK whose 64 ranges span 2^40 counters must return in bounded time.

### Interaction with §8.7's classes and ruling 113's FIN flag
| §8.7 class | `SentFrame` variant | on loss | on ack |
|---|---|---|---|
| `ranges` (STREAM) | `Stream { r, range, fin }` | `on_lost_range(now, r, range, fin)` — returns to `retransmit`, minus already-acked sub-ranges | `on_ack_range(now, r, range, fin)` |
| `regenerate` (RESET_STREAM) | `ResetStream { r }` | `SendHalf::on_reset_lost()` — the identity re-queues, the **current** values are re-read | `on_reset_acked(now, r)` |
| `regenerate` (credit) | `MaxData` / `MaxStreamData { r }` / `MaxStreams { dir }` | set `Regenerate`'s bit; the retransmission carries the **freshest** value | clear nothing — a credit frame is superseded, not confirmed |
| `never` (PING, ACK, PADDING, CLOSE, DATAGRAM) | no variant | nothing | nothing |

**Ruling 113's FIN flag is the reason the `Stream` variant carries `fin`
rather than deriving it.** `Connection::frame_carried_fin` (`mod.rs:544`)
computes `streams.final_size(r) == Some(range.end)`, which is exact for every
frame this implementation emits *today* but not in general: §8.7 lets a
retransmission split, merge or coalesce ranges freely, so a frame ending at the
final size need not have carried the FIN. Once the map exists the answer is
recorded, and `frame_carried_fin` is deleted rather than left as a fallback —
a fallback would keep the wrong answer reachable.

**Also required and easy to miss:** `SendHalf::on_lost_range(range, fin)`
clears `fin_sent` when `fin` is true (`send.rs:437–439`). The FIN must be
recorded on **exactly** the packet that carried it, or a lost packet that
happened to end at the final size resets `fin_sent` and the FIN is re-sent
forever, or — worse in the other direction — the packet that really carried it
does not clear the flag and the FIN is never re-sent, hanging the peer's reader
at EOF. This is what T-FIN-A/T-FIN-B pin from the map's side, and what S12-1's byte-equality assertion catches from the wire's side.

---

## §6. The five debts

### 6.1 Ruling 128 — the post-death drain

**Lands in 5b. Closed by un-ignoring `tests/spec_streams.rs:690`
(`a_receiver_can_drain_a_stream_the_sender_closed_behind`).**

Implementation: §2.6 above — the core `read` guard, and the shell precedence
change for `read` and `accept_*` only.

**Finding — ruling 128's rationale names a guard that does not fire on the path
its own test exercises. Working rule 11, and working rule 12's shape in the
same text ruling 128 used to overturn ruling 118.**

Ruling 128 (rulings.md:3230–3237) says two guards hide the data:

> (a) `core::Connection::read`'s unconditional `self.lost` check, and (b) the
> shell releasing the core at `ToEndpoint::Retired` (`driver.rs:650`), which
> `drop_state` emits **at the instant of death**. Moving (b) means touching
> ruling 81's definition of when `Retired` fires …

Guard (a) is real and fires: `mod.rs:467–469`.

Guard (b) does not fire at the instant of death on the draining path, which is
the path the test builds:

1. The receiver takes `Frame::Close` while live (`mod.rs:638–651`):
   `emit_closed(PeerClosed)`, then `lifecycle = Draining { until: now +
   CLOSE_LINGER }`, then `enter_post_mortem_timers()`, which arms `CloseLinger`
   at `until`.
2. `drop_state()` — the only emitter of `Retired` — is called from exactly two
   places (`mod.rs:303` and `mod.rs:807`). On the draining path it is the
   `TimerKind::CloseLinger` arm, i.e. **`CLOSE_LINGER` = 5 s later**, not at
   the death.
3. The shell's release gate is `cell.closed.is_some() && !cell.is_established()`
   (`driver.rs:634`), and `is_established()` reads `core.is_some_and(…)` →
   `session.is_some()` (`shared.rs:243–247`). The session is taken by
   `drop_state`, so the release at `driver.rs:650` is also 5 s out.

Two further corrections of fact, both cheap and both load-bearing for anyone
reading the ruling later:

- **`driver.rs:650` is not where `Retired` is handled.** `Retired` is consumed
  in `Driver::serve_connection` at `driver.rs:471–482`; line 650 is
  `record.cell.borrow_mut().core = None` inside `Driver::release_dead`, which
  is the *release*, correctly ordered *after* the `Retired` delivery.
- **`drop_state` does not need to change**, and neither does ruling 81.

**What actually blocks the test**, all three shell- or core-side precedence
checks and none of them `Retired`:
- `Connection::poll_accept_with`'s ruling-118 latch-first check
  (`connection.rs:407`) — `accept_uni()` answers `Err(PeerClosed)`;
- `RecvStream::poll_read`'s ruling-124 step 2 (`stream.rs:558`);
- `core::Connection::read`'s `lost` check (`mod.rs:467`).

**Consequence for the plan, and it reduces risk substantially:** the piece of
slice 5 that sounded most dangerous — re-timing `Retired`, which reaches the
endpoint's index tables and the timestamp guard's entry pin — **is not
required**. What is required is three localised precedence changes and a core
guard. This is reported, not resolved: it contradicts a ratified ruling's
stated rationale, and §10-Q2 is the decision it forces.

### 6.2 Ruling 113 — the FIN flag in `on_ack_range`

**Lands in 5a.** `Connection::on_ack_range(now, r, range, fin)` and
`on_lost_range(now, r, range, fin)` take the flag; `frame_carried_fin` is
deleted; the flag comes from `SentFrame::Stream { fin }`. The brief offers
"call `SendHalf::on_ack_range` directly **or** restore the flag to the
`Connection` signature"; **restore the flag** — calling the half directly would
bypass `Streams::on_ack_range`'s flow-ledger and event work
(`streams.rs:370–377`), which is what makes an ACK able to fully close a stream
and owe a MAX_STREAMS grant, and that is exactly why ruling 113 put the verbs on
`Connection` in the first place.

Closed by two tests in `tests_recovery.rs`: **T-FIN-A**, a retransmission
re-framed to end at the final size **without** the FIN does not set
`fin_acked`; **T-FIN-B**, the packet that did carry the FIN does. The degenerate
build (inference) passes T-FIN-B and fails T-FIN-A — see §7.

### 6.3 Ruling 129 — the reachable positive control

**Lands in 5a.** Ruling 129: `reset()` after `finish()` is a no-op at the peer
whenever the final sizes agree, and in slice 4 they always agree, because
ruling 111 pins `final_size` at the highest byte **transmitted** and slice 4
never holds accepted-but-unsealed bytes. The congestion window creates that
state.

The test: fill `cwnd` so a subsequent `write()` is accepted into send state but
not sealed; `finish()`; `reset(code)`. `final_size` is then the highest byte
**transmitted**, strictly below `write_offset`. The peer, which has a FIN
pinning the larger final size, must answer `FINAL_SIZE_ERROR` — the divergence
ruling 129 says is reachable only here. It is the positive control for
`an_explicit_reset_after_finish_supersedes_the_fin`.

**This test depends on §10-Q4** (does `write()` accept past the congestion
window?). If Q4 is answered "block", the state is still unreachable and slice 5
owes ruling 129 nothing but a note — which would itself be a finding, since
ruling 129 asserts it *is* reachable here.

### 6.4 Slice 4a's three watermark tests, plus §10.4's scope rule

**Lands in 5a**, in `tests_recovery.rs`, because all four became reachable for
one reason: a locally-opened stream fully closes only on acknowledgement
(rulings.md:2620–2634).

1. **legality-before-watermark on a closed local-uni index** — a STREAM or
   RESET_STREAM naming an index we opened and have since fully closed is a
   no-op (§8.4: *"ACKed, never re-opened"*), not a `STREAM_STATE_ERROR`.
2. **"credit for a fully-closed stream is a valid no-op"** (§8.4) on a stream
   we can send on.
3. **a locally-opened watermark advancing at all** — ruling 97's H3, undefended
   by anything writable in 4a.
4. **§10.4's RFC 9000 §4.6 scope rule** — *"closing streams we opened must not
   inflate the peer's allowance"*: acknowledging our own stream's closure
   advances **our** watermark and grants the peer nothing.

Ruling 97's named test was *reduced* in 4a and says so in its own doc comment;
slice 5 restores the watermark clause. Working rule 9 applies hardest to (3):
a build that never advances the local watermark passes 4a's 73 tests, so the
test must assert the watermark **moved**, from the side that separates it —
compare before and after, not "is at least".

### 6.5 `credit_frames_precede_the_stream_fill_in_a_packet`

**The brief says this one is slice 7's and asks me to confirm. I cannot
confirm it — it is slice 5's, and the `#[ignore]` reason is wrong.** Working
rule 5.

The attribute reads `#[ignore = "needs §14's congestion bound to create pending
stream data (slice 7)"]` (`tests_streams.rs:3234`), and its doc comment says
*"§14's congestion window creates exactly that state, which is why this is slice
7's"*. §14 is **slice 5's** — `PLAN.md`'s slice table, row 5: *"§12 ACK fused to
the replay window + delayed ACK, §13 RFC 9002, §14 NewReno"*. Slice 7 is
"Mobility & contest": §7.3 roaming, §7.5 keepalive and the contested probe,
§5.4/§6.4/§6.7–6.8, `notified()`. **§14 does not appear in slice 7's row at
all.**

The mislabel is not local to this test — it has a traceable source and it has
propagated. See §9-C3. The test becomes reachable the moment §14's admission
gate can leave stream data pending across a mutating call, which is the same
condition ruling 129 needs (§6.3) and the same condition §8-H1 describes. **It
should be un-ignored in 5a, by the integrator, and it should go green.** If it
does not, that is a finding about the gate's placement, not about the test.

I have not adopted it into 5a's test-authoring scope — a blind author must not
be handed a test written by someone else. It is an **integrator's exit check**
for 5a: un-ignore, run, and report.

---

## §7. Story-to-test mapping

Working rule 9 governs this whole section: *a bound is only a test if the
degenerate case violates it.* Slice 2a shipped two tests named for properties
they failed to pin, and a congestion controller is the most prone construct in
the project — "the window grew" passes against almost anything. **Every test
below states what the broken build does.** Where a test cannot separate the
readings, it is not written (rulings.md:2681–2686: not writing a test is the
harder call and the right one).

### S12 — a user can stream, full, over `FlakyWire` with loss on
> *Accepts:* open a stream, write, the peer reads the same bytes in the same
> order with no gaps or duplicates, `finish()` delivers the FIN, the reader
> observes end-of-stream. **Survives loss, reordering and duplication on the
> path.** Anchor §9, §10, §11, §12, §13. Paused clock, over `FlakyWire`.

`tests/story_reliability.rs`, blind author 5b-test.

| id | test | what the **broken** build does |
|---|---|---|
| S12-1 | 256 KiB over `FlakyPolicy::lossy(0.10).with_delay(20 ms, 15 ms).with_duplication(0.05)`, seeded; peer reads byte-identical content and `Ok(None)` | a build with no retransmission **hangs** — the reader never reaches EOF and the test times out under `within()`. A build that retransmits but does not dedup by offset delivers **corrupted** bytes (the assert is byte-equality, not length) |
| S12-2 | the same transfer with `drop_first(N)` set **after** `establish()` so the first N *data* packets are lost outright | a build that arms `Pto` only when a `Loss` timer is absent, or that never arms `Pto` on an empty-ack history, hangs: nothing has ever been acknowledged, so `largest_acked` is `None` and §13.2's walk declares nothing. **Only the PTO can rescue this**, which is what makes it a pin on §13.3's arming rule rather than on loss detection |
| S12-3 | reordering only (`with_delay(1 ms, 60 ms)`, no loss); assert the transfer completes **and** that the receiver sent at least one immediate ACK | a build implementing only the "every 2nd" trigger and not §12.4's out-of-order trigger still completes — so completion alone asserts nothing. The pin is the ACK count against a lower bound derived from the gap count |
| S12-4 | permanent one-way blackhole (`block_path`) mid-transfer, healed after 3 × PTO; transfer completes | a build whose PTO backoff resets on its own probes (rather than on acknowledgement) sends a probe train at a fixed interval and completes anyway — so the assertion must be on the **probe timestamps**, not on completion: intervals must not all be equal. This is slice 2a's jitter defect in a new costume |
| S12-5 | S13's no-head-of-line-blocking under loss: two streams, loss on the path, assert stream B completes while stream A is still retransmitting | a build that re-queues a lost range at the **head** of the rotation starves B and fails; a build with no rotation at all also fails. `push_front` vs `push_back` is the mutation (ruling 114 used exactly this one) |

### S28 — wait until what was sent is acknowledged, then close
> *Accepts:* the application can await transport-level acknowledgement of what
> it has sent, then `close()` without loss. **Paused clock: yes — send, await
> acknowledgement, close, assert the peer received it, with loss injected.**

**S28's stated shape is `send_message(msg).await; acked().await; close(…)`,
and `send_message` is §9.8 — slice 6.** Slice 5 closes S28 over a **stream**
(`open_uni` → `write` → `finish` → `acked` → `close`), which is the same
mechanism through the same core event; the `send_message` form is re-verified
in slice 6. Recorded here rather than discovered there.

| id | test | what the **broken** build does |
|---|---|---|
| S28-1 | write 4 KiB, `finish()`, `SendStream::acked()`, `close(NO_ERROR)`; peer reads all 4 KiB and EOF. Loss at 20 % | a build whose `acked()` resolves on `finish()` (the tempting one-liner) closes while data is unacknowledged, §15.2 drops the recovery state, and the **peer's read is short**. The assertion is on the peer's bytes, not on `acked()` returning |
| S28-2 | the same, with `Connection::acked()` and two concurrent streams; both must be delivered before the close | a build snapshotting only the stream most recently written passes S28-1 and fails here |
| S28-3 | `Connection::acked()` on a connection with **nothing written** resolves `Ok(())` immediately, on a live and on a dead connection | a build that parks until a `StreamFinished` that will never come hangs. Cheap, and it is the shape an application hits by writing `acked()` in a shutdown helper |
| S28-4 | `acked()` terminates on a live connection **while a bulk stream is still being written** (§16.2:4381–4383) | a build whose snapshot is "all streams' current offsets, re-read at each poll" never terminates under a writer loop. This is the clause §16.2 states and the one a plausible implementation gets wrong |
| S28-5 | `SendStream::acked()` returns `Err(Reset(code))` when the stream is reset before its data is acknowledged | a build that only ever resolves `Ok` hangs |

### §12 — `tests_ack.rs` (5a-test-ack), the pins that matter

| id | test | what the **broken** build does |
|---|---|---|
| A-1 | after 2 ack-eliciting packets an ACK is emitted; after 1 it is not, until `AckDelay` fires at exactly `+25 ms` | an immediate-ACK-per-packet build (the pre-ratification policy) fails the "after 1, none" half. A delay-only build fails the "after 2" half. **Both halves are needed**; either alone is passed by a wrong build |
| A-2 | an out-of-order arrival (counter ≠ prev_greatest + 1) yields an ACK **in the same drain**, even as the 1st since the last ACK | a build implementing only the counter and the timer passes every other §12 test |
| A-3 | `ack_delay` is the measured µs from the arrival of the packet bearing `largest`, and is **0** when that packet was a keepalive | a build reporting 0 always passes any "ack_delay is small" bound; the assertion is equality against the injected delta |
| A-4 | a 2048-counter alternating window truncates at **64 pairs** and at packet capacity, newest-first | a build that emits the oldest ranges first passes a "≤ 64 pairs" bound and loses exactly the ranges §12.2 says are least likely to be already carried. Assert the **first** block covers the greatest |
| A-5 | `range_count = 65` on receipt is structural: nothing from the packet applies, CLOSE with `PROTOCOL_VIOLATION`; `range_count = 64` applies normally | the one-sided-boundary defect from slice 1: test **both** sides |
| A-6 | an ACK whose `largest` exceeds the highest counter sealed is ignored **whole**, and the other frames in that packet still apply | a build that treats it as a violation kills the connection; a build that applies it corrupts the map. The pin is the *other* frame's effect being visible |
| A-7 | a pure-ACK packet is sealed `seal_quiet`, is not ack-eliciting, is **not** in the sent map, and does not move `bytes_in_flight` | a build that tracks pure ACKs grows `bytes_in_flight` monotonically and eventually gates itself to a halt — but only after minutes, so nothing else catches it |
| A-8 | a wire-legal ACK with 64 ranges spanning 2⁴⁰ counters returns in bounded time and acknowledges only what is in flight | a materialising build allocates and either OOMs or takes minutes. Assert the resulting `bytes_in_flight` delta, not the wall time |
| A-9 | a **replayed** packet does not advance §12.4's counter | a build folding replays in emits an extra ACK per replay — free reverse-path amplification, and invisible to every other test |

### §13/§14 — `tests_recovery.rs` (5a-test-recovery)

| id | test | what the **broken** build does |
|---|---|---|
| R-1 | first RTT sample sets `srtt = latest`, `rttvar = latest/2`; the second follows ⅞/⅛ and ¾/¼ **exactly** (assert equality on the `Duration`) | a build using `smoothed = latest` every time passes any "srtt is near the true RTT" bound |
| R-2 | the peer's `ack_delay` is subtracted **only** when the result stays ≥ `min_rtt` | a build that always subtracts drives `srtt` below `min_rtt` and shortens the PTO — which no completion test detects |
| R-3 | before any sample, `srtt = 333 ms` and `rttvar = 166.5 ms`, and the first PTO is exactly `333 + max(4·166.5, 1) + 25 = 1024 ms` | a build seeding `rttvar = 0` gives 358 ms, still "plausible" |
| R-4 | packet threshold is **exactly 3**: 3 below largest-acked is lost, 2 below is not | one-sided |
| R-5 | time threshold declares a straggler at `9/8 · max(srtt, latest)` and **not** at `8/8` | a build using `srtt` alone (dropping the `max` with `latest`) passes on a stable path and mis-declares on a rising one |
| R-6 | packets **above** `largest_acked` are neither declared lost nor allowed to arm the `Loss` timer | see §9-C1 — this test is written **only after** C1 is ruled |
| R-7 | PTO backoff doubles: 1×, 2×, 4×, 8× of the base, and **caps at 64×**, never 2⁶⁴ | the `1 << PTO_BACKOFF_CAP` transcription (§4.3) either panics in debug or produces a deadline `Instant` addition overflow |
| R-8 | `pto_count` resets to 0 on **any** newly acknowledged packet | a build resetting only on the probe's own ack keeps the backoff after recovery |
| R-9 | the `Pto` timer is **disarmed** when the sent map empties | a build that leaves it armed self-sustains a probe train at ~20 packets/s (§13.3's own stated failure) — invisible to any completion test, visible as a packet count |
| R-10 | a loss episode cuts `cwnd` to exactly `max(cwnd/2, 2400)` **once**; a second loss of a packet sent **before** `recovery_start` does not cut again | a build cutting per lost packet collapses to `MINIMUM_WINDOW` on any burst — and still completes every transfer, slowly. Assert the exact value |
| R-11 | acknowledgements of packets sent **before** `recovery_start` do not grow `cwnd` (§14.3's symmetric half) | a build implementing only the event half re-inflates through the recovery it triggered. Assert `cwnd` **unchanged** across those acks |
| R-12 | slow start adds exactly the bytes acknowledged; congestion avoidance adds exactly one `MAX_DATAGRAM` per accumulator crossing, with the remainder carried | a build using `if` instead of `while` under-grows on a large ack burst; a build not carrying the remainder over-grows. Assert exact `cwnd` after a scripted ack sequence |
| R-13 | persistent congestion collapses `cwnd` to `MINIMUM_WINDOW` when two losses **more than `3 × pto_base(pto_count = 0)`** apart have nothing acked between them — and does **not** when `pto_count > 0`'s backoff is included in the period | a build using the backed-off PTO computes a period up to 64× too long and **never** triggers, under exactly the loss it exists to detect (§14.4 says this in terms). The test must script the backoff so the two periods differ |
| R-14 | persistent congestion does **not** trigger before the first RTT sample | a build without §14.4's `has_sample()` guard collapses the window on a first-flight blackhole |
| R-15 | the gate is `bytes_in_flight + size <= cwnd`: with `cwnd = 12 000` and 1200-byte packets exactly **10** are admitted and the 11th is not | a build using `<` admits 9; a build gating on plaintext rather than datagram size admits 10 and overshoots the window by 10 × 30 bytes |
| R-16 | a PTO probe is sent **with the window full** | a build applying the gate uniformly deadlocks a black-holed path forever — and no completion test on a *healing* path detects it |
| R-17 | that same probe **is** in the sent map and **does** count in `bytes_in_flight` (ruling 43, §17.5) | a build exempting it from accounting as well as admission puts a packet in flight recovery cannot see. Assert the delta, not the send |
| R-18 | `app_limited` recorded on a packet sent with headroom to spare suppresses growth on **that** packet's ack, and a packet sent under a full window does not | a build never setting the flag grows the window while idle — which every throughput test rewards |
| R-19 | `bytes_in_flight` returns to **0** after every packet is acknowledged | a build leaking entries gates itself shut after minutes |
| T-FIN-A / T-FIN-B | §6.2's pair | inference passes B, fails A |

**Two tests deliberately not written**, with reasons, per 4a's precedent:
- **the roam fences** (§13.6/§14.6). Roaming is slice 7's; the fences would be
  tested through `on_roam`/`reset` called by nothing, which asserts the method
  exists and not that the protocol does. §10-Q9 is the decision that matters
  here, and it is a schema question, not a test question.
- **§12.2's ACK-fidelity failure mode** (§7.2's ACK-loss burst longer than the
  window's time width). The fixture cannot produce it — see §11-F2.

---

## §8. The unstated-scope hunt

*A stated construction with an unstated or contradicted scope.* Nineteen
instances across six slices, never once a wrong value. Fourteen candidates
below; the first is the largest.

**H1 — `write()`'s meaning changes, and no section says so.** Ruling 114
ratified that *"in slice 4, with no congestion bound, a mutating call flushes
everything the ledger admits"*, and ruling 129 built on the consequence that
*"a blocked write returns `Pending` and the bytes stay with the caller"*. Slice
5 adds the bound. **Nothing in §14, §16.2, §16.7 or §10.6 states what `write()`
does when flow control admits the bytes and the congestion window does not.**
§14.5 gates *sends*; §10.6 makes credit the *buffer* commitment; §16.7 places
the seal. Two readings: `write()` accepts into send state and the bytes wait
(QUIC's, and the one ruling 111/129 require), or `write()` returns `Ok(0)` and
parks (slice 4's, and the one that keeps `write` a byte-ledger verb). Bounded
either way — credit bounds the buffer — but they differ in every observable
this slice touches. **§10-Q4.**

**H2 — §13.1's second sample condition is vacuous, and reads as a requirement
to track what §13.5 forbids tracking.** §13.1: a sample is taken when `largest`
is newly acknowledged *"and at least one newly acknowledged packet is
ack-eliciting"*. §13.5: *"Non-ack-eliciting packets are never inserted."* Every
packet in the map is ack-eliciting, so the second clause can never be false. It
is RFC 9002 §5.1's wording carried across from a design that tracks both kinds.
An implementer reading §13.1 as exhaustive may build the non-ack-eliciting
tracking to evaluate it. **Recommend: state in the contract that the clause is
vacuous and MUST NOT be implemented.**

**H3 — `latest_rtt` is kept but never defined.** §13.1 names the four
quantities the estimator keeps and gives update rules for three. `latest_rtt`
is `ack_arrival − time_sent(largest_newly_acked)` in RFC 9002 §5.1; §13 never
says so, and it is also the input to §13.2's `max(smoothed_rtt, latest_rtt)`.
**Answerable only by importing the RFC**, so it belongs in the contract.

**H4 — which `largest`?** §13.1 says a sample is yielded when "its `largest` is
newly acknowledged". If the ACK's `largest` was already acknowledged by an
earlier ACK but the frame newly acknowledges *other* packets, there is no
sample. That is RFC 9002's rule and it is what "its `largest` is newly
acknowledged" means, but "newly acknowledged" is doing load-bearing work
unstated. Recommend stating it.

**H5 — when does `pto_count` increment?** §13.3 gives the reset ("whenever any
packet is newly acknowledged") and the cap, and says the interval is "doubled
per consecutive unanswered probe". The increment moment — at the timer's firing,
before the probe is built — is unstated. §10-Q5.

**H6 — does a probe re-anchor `last_ack_eliciting`?** The PTO is "anchored at
the last ack-eliciting send", and a probe **is** an ack-eliciting send. So the
next PTO is anchored at the probe, with `pto_count` now 1 — which is RFC 9002's
behaviour and is what makes the train's intervals `1×, 2×, 4×…` rather than
`1×, 3×, 7×…` from a fixed anchor. Unstated, and the two readings give
observably different probe timestamps (S12-4 and R-7 both depend on it).

**H7 — does persistent congestion clear `recovery_start`?** §14.4 says the
controller "collapses: `cwnd = MINIMUM_WINDOW`, slow start effectively
restarts". RFC 9002 §7.6.2 additionally clears the recovery-start marker.
Leaving it set fences every pre-collapse packet's ack from growing the window;
clearing it lets them. §10-Q6.

**H8 — the packet's "size in bytes".** §13.5 and §14.5 both use it and neither
says whether it is plaintext, ciphertext or datagram. All three are defensible
readings and they differ by 30 bytes/packet — 2.5 % of a window. §10-Q3.

**H9 — `app_limited`'s moment.** §14.5: "set when the sender runs out of queued
data with cwnd headroom remaining — and **records it onto each sent packet**".
Whether the packet that exhausted the queue carries the flag, or only packets
sent *after* the sender went idle, is unstated. §10-Q7.

**H10 — §12.2's "there is no second tracker" versus §12.3/§12.4's state.**
§12.2 says the window is "the single received-packet record". §12.3 needs the
arrival instant of the packet bearing `largest` and whether it was frame-seen;
§12.4 needs a count of ack-eliciting packets since the last ACK and an owed
flag. Those are four scalars the window does not hold. They are not a *record*
— they do not scale with packets received — so the sentence is true as written
and misleading as read. Stating the boundary ("no second per-packet record; the
scalars alongside are not one") costs one sentence in the contract and prevents
an implementer either duplicating the window or trying to derive `ack_delay`
from it.

**H11 — the `Loss` timer's candidate set.** §13.2's "(minimum across in-flight
packets)" versus "Survivors inside the threshold". Escalated to §9-C1 because
the two readings are not reconcilable by scope-stating alone.

**H12 — §12.4's counter reset and the `AckDelay` disarm.** *Answerable from the
spec:* §12.4 arms `AckDelay` "on receipt of the **first unacknowledged**
ack-eliciting packet", so packing an ACK makes all received packets
acknowledged, which resets the counter and disarms the timer. §16.5's
equal-deadline list confirms the direction — `AckDelay` fires *after* the
loss/PTO evaluation so the owed ACK "rides any probe or retransmission that
evaluation produced". Stated in the contract, not raised as a question.

**H13 — is a bare-PING probe inserted into the map?** *Answerable:* §13.5 says
"Non-ack-eliciting packets are never inserted"; §8.3 makes PING ack-eliciting;
§17.5's caveat says in terms that both exempt probes are inserted and counted.
So yes, with an empty `frames` vector. Stated in §2.2, not raised.

**H14 — §14.1's trait visibility and where `Controller` may be implemented
from.** §14.1 presents the trait unqualified and says CUBIC/BBR are "pure
additions behind the trait later (§19)". "Later" bounds it out of v1's public
surface, but the text does not say so. §10-Q8.

---

## §9. Conflicts — reported, not resolved

Working rule 3: when two statements conflict, do not default to the code-like
rule. Three times in this project the prose held the correct intent and the
formal rule held the bug.

**C1 — §13.2's `Loss` timer candidate set.** SPEC.md:3685–3687:

> Survivors **inside the threshold** arm the `Loss` timer at
> `time_sent + loss_delay` (**minimum across in-flight packets**).

"Survivors inside the threshold" are, by the two preceding bullets, packets
below `largest_acked` that failed both tests. "Minimum across in-flight
packets" is every entry in the map, including those *above* `largest_acked`,
which the walk does not judge at all. The two readings differ whenever anything
newer than `largest_acked` is outstanding, which is the ordinary case during a
transfer: the second arms a timer that fires and declares nothing (a spurious
wakeup at best), and an implementation that then *acts* on it declares recent
packets lost. RFC 9002 §6.1.2 and v0.1 (`recovery.rs:466–470`, `if pn >
largest_acked { continue }`) both take the first reading. **The prose is the
narrower and, I believe, the intended rule; the parenthetical is the
code-shaped one. Reporting, not choosing.** R-6 is written only after this is
ruled.

**C2 — §13.1's ack-eliciting sample condition versus §13.5's map contents.**
SPEC.md:3650–3651 versus SPEC.md:3745. See §8-H2. Not a contradiction of
outcome; a contradiction of what state must exist. Both sides quoted above.

**C3 — three rulings and three shipped doc comments place §13/§14 in slice 7;
`PLAN.md` places them in slice 5.**

| source | says |
|---|---|
| `PLAN.md` §4, row 5 | *"**Reliability** — §12 ACK …, §13 RFC 9002 …, §14 NewReno, ruling 47's `acked()`"* |
| `PLAN.md` §4, row 7 | *"**Mobility & contest** — §7.3 roaming …, §7.5 keepalive …, §5.4 / §6.4 / §6.7–6.8 …, `notified()`"* — **§14 does not appear** |
| ruling 105 (rulings.md:2418–2420) | *"§12 is slice 5 and **§13 is slice 7**"* |
| ruling 114 (rulings.md:2700–2702) | *"§14's congestion window is **slice 7**"*; and :2745 *"the seam to **slice 7** is one call site"* |
| `.slices/04-streams/PLAN.md:1903` | *"§12.6 §14 — congestion control (**slice 7**)"* ← the source |
| `send.rs:213–215` | *"**slice 7's** congestion window inserts a second bound there"* |
| `tests_streams.rs:3234` | `#[ignore = "needs §14's congestion bound … (**slice 7**)"]` |
| ruling 129 (rulings.md:3265–3266) | *"which is **slice 5's** congestion window"* — the one that agrees with `PLAN.md` |

Two inherited debts move with the answer: **ruling 105's loss-driven tombstone
variant** (*"free a stream, drop the ACK, let the peer's PTO retransmission
re-name it"*) needs §12 **and** §13, both slice 5's under `PLAN.md`; and
**ruling 113's carry of ruling 98's STREAM-retransmission seal row** — the row
where one frame type takes two different seals — needs a *retransmission*,
which is §13's, i.e. slice 5's. The brief asks whether §12 forces ruling 98's
row earlier; the answer is that §12 does not, but **§13 does, and §13 is slice
5's**. Both are therefore slice 5 debts under `PLAN.md`'s reading and slice 7
debts under the rulings' reading. `PLAN.md` is the approved plan and ruling 129
agrees with it; three earlier texts do not. **Reporting.**

**C3a — a second-order effect worth its own line: seven doc comments in
slice-4 core code cite "§12.x" section numbers that are `.slices/04-streams/
PLAN.md`'s, not `SPEC.md`'s.** `send.rs:18` ("§12.5's seam"), `send.rs:60`
("§12.1's seam"), `send.rs:213` ("**§12.6's seam**"), `send.rs:402`, `send.rs:740`,
`streams.rs:141`, `streams.rs:166`, `streams.rs:365`, `streams.rs:544`
("§12.7: *credit frames apply as O(1) monotone-max*" — that quote is
`SPEC.md:3418`, i.e. **§10.6**). SPEC's §12 has exactly five subsections
(SPEC.md:3555–3624) and no §12.6 or §12.7. In a project where CLAUDE.md makes
`SPEC.md` the authority and "§N" reads as a spec citation, these are dangling —
and they are dangling **in slice 5's own files**, pointing at SPEC's §12, which
is slice 5's subject. Cheap to fix; expensive to be misled by.

**C4 — ruling 128's guard (b) versus the code.** Full workings at §6.1. Ruling
128 (rulings.md:3234–3237) says the shell releases the core at `Retired`, *"which
`drop_state` emits at the instant of death"*, and that moving it *"means touching
ruling 81's definition of when `Retired` fires"*. On the draining path —
the path its own test builds — `drop_state` runs at `CloseLinger` expiry
(`mod.rs:303`, armed at `mod.rs:645–647`), so the release is 5 s after the death
and ruling 81 need not be touched. The ruling's conclusion is unaffected; its
scoping decision was priced against work that is not required. **Reporting**,
because it is a ratified ruling's rationale and because it changes slice 5's
risk profile downward.

**C5 — §14.6's single marker cannot serve §13.6's four fences.** §14.6:
*"**The recovery-period marker is set to the roam instant — not cleared**"*.
§13.6 lists four things pre-roam packets must not feed: **congestion event**,
**persistent-congestion walk**, **RTT sample**, **`app_limited` growth**. The
recovery marker (`sent_time <= recovery_start`) serves the first and the
fourth, because §14.3 already gates both on it. It cannot serve the RTT fence:
`recovery_start` is *also* set by every ordinary congestion event, so an
implementation reusing it would suppress RTT sampling after every normal loss
episode — silently, and forever on a lossy path. §14.6 names "quinn's
path-generation stamping" in the same breath, which is the mechanism that does
work. **The spec presents one marker as doing a job that needs two.** Reporting;
§10-Q9 is the slice-5 decision it forces.

**C6 — §15.2's exhaustive retention list versus ruling 128 versus the code.**
§15.2 (SPEC.md:3945–3947): the closing state retains *"the seal capability, the
receive cipher states, and the replay window (all stream, flow-control,
recovery, and congestion state may drop immediately …)"* — and `close.rs:83–88`
restates it as exhaustive. Ruling 128 requires received stream state to survive
the death at the **receiver**, and §15.2's *receiving* bullet permits it ("hold
a brief drain … then drop all state"). The code retains stream state on **both**
paths, because `drop_state` leaves `streams` and `flow` untouched — which is
what makes ruling 128 implementable and what makes the *closing* endpoint hold
memory §15.2 says it may release. Slice 5 must decide whether the closing path
frees `streams`/`flow` (§15.2's letter, and the memory bound §17.5 assumes) or
retains them (the code today, and one fewer branch). **Reporting.**

**C-v01 — v0.1's arithmetic versus §13, one difference.** v0.1's `detect_lost`
uses `by_time = now.saturating_duration_since(sent.time_sent) >= loss_delay`
(`recovery.rs:474`), i.e. **`>=`**; §13.2 says *"it was sent **more than**
`loss_delay` before the acknowledgment arrived"*, i.e. **`>`**. §13 wins per the
brief's scope rule. The difference is one `K_GRANULARITY` tick at the boundary
and is invisible except to a test that lands exactly on it — which R-5 should,
one-sidedly, on both sides. Reported per the brief's instruction to report v0.1
divergences rather than absorb them.

---

## §10. Open questions, ranked by cost of a wrong answer

**Q1 (highest) — does `Connection::acked()`'s settled snapshot outrank the
connection's death latch?**
*Candidates:* (a) settled-first, then the latch; (b) latch-first (ruling 118's
original shape).
*Cost if (b) and wrong:* `write; acked(); close()` — the exact sequence S28
exists for, and the one §16.2:4384 spells out — answers `Err(ConnectionLost)`
when the peer's ACK and the peer's CLOSE arrive in the same driver pass, over a
transfer that was fully delivered. **This is ruling 128's defect on the
sender's side**, reachable by the same mechanism (one pass processes both, the
latch is set before the application is woken) and it is not a race the sender
can win. *Cost if (a) and wrong:* an application learns of the death from
`closed()` a moment later than it might have; nothing is lost.
**Recommend (a)**, by ruling 124's stated principle — *"a stream that reached
EOF completed, and the connection dying afterwards does not un-complete it"* —
which is the same sentence with "acknowledged" for "EOF". Same for
`SendStream::acked()`.

**Q2 — what is ruling 128's drain window?**
*Candidates:* (a) while the shell holds the core — `CLOSE_LINGER` (5 s) on
close/draining paths, **zero** on `die()` paths (liveness timeout, nonce
exhaustion, `Replaced`, `EndpointDropped`); (b) until the last handle drops,
gating `release_dead` on `handles == 0` as well.
*Cost if (a) and wrong:* a receiver killed by `DEAD_TIMEOUT` mid-stream cannot
drain, even though the bytes arrived. But that path has no CLOSE, so the sender
was not finished either — the loss is real, not an artefact. *Cost if (b) and
wrong:* a leaked handle pins a dead connection's receive buffers indefinitely,
which §17.5's ceiling table does not cover.
**Recommend (a)**, and document the window on `RecvStream::read` and
`Connection::accept_*`. It requires no change to `release_dead`, no change to
`Retired`, and no change to ruling 81 — see §6.1 and §9-C4. State the `die()`
consequence explicitly rather than leaving it to be discovered.

**Q3 — what is a packet's `size` (§13.5, §14.5)?**
*Candidates:* (a) the full datagram, `DATA_HEADER_LEN (14) + ciphertext +
AEAD_TAG_LEN (16)` = `Transmit::data.len()`; (b) the plaintext; (c) the
ciphertext.
*Cost if wrong:* (b) under-counts by 30 B/packet — with `cwnd = 12 000` and
1200-B datagrams that is 10 packets admitted against a true 12 300 B in flight,
a 2.5 % standing overshoot that grows with the window and is invisible to every
functional test.
**Recommend (a)**, and it is nearly answerable from the spec: `INITIAL_WINDOW`
is derived as `min(10 × 1200, max(2 × 1200, 14 720))` *"at `MAX_DATAGRAM` =
1200"* (SPEC.md:3796), and `MAX_DATAGRAM` is the **datagram** size
(`constants.rs:138`, `MAX_PLAINTEXT = MAX_DATAGRAM − DATA_HEADER_LEN −
AEAD_TAG_LEN`). A window expressed in datagram units must be spent in datagram
units. Cite SPEC.md:3796 in the contract.

**Q4 — does `write()` accept bytes the congestion window cannot yet send?**
*Candidates:* (a) yes — flow control admits, the bytes enter send state, the
gate defers only the *seal*; (b) no — `write()` returns `Ok(0)` and the shell
parks until window room appears.
*Cost if (b) and wrong:* ruling 129's positive control stays unreachable
(contradicting ruling 129, which asserts slice 5 makes it reachable);
`credit_frames_precede_the_stream_fill_in_a_packet` stays red-or-ignored
forever; ruling 111's "ordinary operation" parenthetical never happens; and the
shell's writer parks on a condition — window room — that no `ConnEvent` wakes,
because `StreamWritable` is credit-driven (`streams.rs`, `send.rs:97–99`), so
slice 5 would owe a new event and a new waker map. *Cost if (a) and wrong:* the
send buffer holds up to the peer's advertised credit rather than up to what the
window admits — which §10.6 already permits and already bounds.
**Recommend (a)**, strongly. It is what ruling 111 and ruling 129 both assume,
it is QUIC's model, §10.6 already provides the bound, and (b) requires
inventing a wakeup path the spec does not describe. It is nonetheless the
largest behavioural decision in the slice (§8-H1) and should be ruled rather
than assumed, because it changes what `write()` means.

**Q5 — when does `pto_count` increment?**
*Candidates:* (a) at the `Pto` timer's firing, before the probe is built;
(b) at the probe's transmission.
*Cost if wrong:* they differ only when the probe cannot be built or sent — in
slice 5, never, since §13.4 falls back to a bare PING and probes are
gate-exempt. In slice 7 they diverge under §7.3's budget, where (b) would stall
the backoff at an unvalidated address.
**Recommend (a)**, matching RFC 9002 and v0.1 (`recovery.rs:522`). Low cost
now, and (a) is the one that stays right in slice 7.

**Q6 — does persistent congestion clear `recovery_start`?**
*Candidates:* (a) clear it (RFC 9002 §7.6.2); (b) leave it at the event.
*Cost if (b) and wrong:* after a collapse to 2 400 B, every packet sent before
the collapse is fenced from growing the window, so recovery from persistent
congestion is slower than slow start by up to one flight — mild, and it
self-corrects. *Cost if (a) and wrong:* the pre-collapse flight's acks grow the
window during the collapse, which is the inflation §14.3's symmetric rule
exists to stop.
**Recommend (b)** — leave it set. §14.4 says only *"slow start effectively
restarts"*, which `cwnd = MINIMUM_WINDOW < ssthresh` already achieves, and
§14.3's rule is stated in slither without RFC 9002's carve-out. Low cost either
way; worth stating so two agents do not resolve it two ways.

**Q7 — `app_limited`: which packets carry it?**
*Candidates:* (a) the packet that emptied the queue while headroom remained;
(b) only packets sent after the sender has already gone idle.
*Cost if (a) and wrong:* one packet per idle transition fails to grow the
window — negligible. *Cost if (b) and wrong:* a bulk sender that keeps the
queue exactly one packet ahead never sets the flag and grows the window while
effectively idle, which is the hostile-peer case §14.5 reasons about.
**Recommend (a)** — §14.5's "records it onto each sent packet" reads as a
property of the send, and (a) is quinn's.

**Q8 — `Controller`'s visibility.**
*Candidates:* `pub` (extensible now) or `pub(crate)`.
*Cost if `pub` and wrong:* a public trait in v0.2 is a semver commitment to a
shape §19 intends to revisit for CUBIC/BBR. *Cost if `pub(crate)` and wrong:* a
consumer cannot plug a controller — which §14.1 defers to "later" anyway.
**Recommend `pub(crate)`**, and say so in the rustdoc so slice 9's API review
does not read it as an oversight.

**Q9 — does `SentPacket` carry a path generation in slice 5?**
*Candidates:* (a) yes — a `u32` stamp, always 0 in slice 5, used by slice 7's
fences; (b) no — slice 7 adds the field.
*Cost if (b) and wrong:* slice 7 changes the map's schema and re-touches every
site that builds a `SentPacket`, in the slice that is already the densest
remaining; and §9-C5's four fences get implemented against `recovery_start`,
which is wrong for two of them. *Cost if (a) and wrong:* one dead `u32` and a
field a blind author will ask about.
**Recommend (a)**, with the field documented as slice 7's and asserted to be 0
in slice 5. §14.6 names path-generation stamping itself; adding the stamp now
costs one field and pre-empts C5's defect.

**Q10 — `SendStream::acked()` before `finish()`.**
*Candidates:* (a) parks (never resolves on a live connection); (b) resolves
`Ok(())` when all *written* bytes are acked, ignoring the FIN; (c) errors.
*Cost if (a) and wrong:* an application writing `write().await;
acked().await;` hangs and looks like a transport bug. *Cost if (b) and wrong:*
`acked()` and `Connection::acked()` mean different things about the same
bytes, and `poll_shutdown` (ruling 57 = `finish()` + `acked()`) loses its FIN
guarantee.
**Recommend (a)**, because §16.2:4369–4370 says "every byte written to that
stream **and its FIN**" and (b) contradicts it — but put the hazard in rustdoc
at the call site, alongside §16.2's own "legal and expected after `finish()`".
(c) is excluded: §18.1 is closed and no variant fits.

---

## §11. Risk register

**F1 (working rule 13, the headline) — `FlakyWire` cannot drop by content, so
the fixture cannot express "lose the ACK".** `testutil` never parses a packet
(`testutil/mod.rs:88–91`); `FlakyPolicy` offers `loss`, `duplicate`,
`base_delay`, `jitter`, `drop_first`, `drop_at`, `send_failure` — all
index- or probability-based, none predicate-based. Loss recovery is *the* place
where an unexpressible loss pattern silently limits a suite, and one inherited
debt lands squarely on it: **ruling 105's loss-driven tombstone variant is
"free a stream, drop the ACK, let the peer's PTO retransmission re-name it"** —
a *content-selected* drop, which `FlakyWire` cannot produce at all.
*Mitigation:* that test belongs at the **core** level, using
`tests_streams::Pair` (two cores over a hand-driven wire, `a_to_b`/`b_to_a` as
held `Vec<Vec<u8>>`), which can withhold an individual datagram by index and
whose datagrams the test can decrypt and inspect via `RawPeer::open_dgram`.
**This must be in the brief**, or the author will attempt it against
`FlakyWire`, fail, and either weaken it or report it as unreachable.

**F2 — the shell fixture cannot produce §7.2's ratified ACK-fidelity failure
mode.** §7.2 ratifies the fused-window design on the argument that only *"an
ACK-loss burst longer than the window's time-width"* causes
delivered-but-unreported packets, and points at an Appendix B simulation. 2048
counters at fixture throughput is minutes of virtual time and thousands of
packets; a `drop_at` enumeration of that shape is not a test, it is a
simulation. **Recorded as not covered by slice 5**, with the Appendix B
obligation intact — an obligation quietly marked done by a weaker test is how a
slice ships a gap (ruling 105's own words).

**F3 — `drop_at` indices are absolute per-wire send counters including the
handshake** (`testutil/mod.rs:582–597`; `set_policy` does not reset `sent`). A
brief saying "drop the 3rd data packet" will produce a test that drops a
handshake packet instead. *Mitigation:* install the policy **after**
`Pair::establish()` and compute the offset from `Network::sends()` /
`Tap::len()`; state the idiom in the brief with a worked line.

**F4 — `Solo` has no clock, and §13/§14 are entirely clock-driven.** `Solo`
drives `core::Connection` directly with `now` as an argument, which is *ideal*
for RTT/PTO/NewReno arithmetic — every instant is chosen, nothing is
approximate — but it means the arithmetic tests can never observe the shell's
lateness bound `L` (§16.5, 250 ms) or the driver's timer plumbing. Conversely
the story tests run under `start_paused` and cannot read `cwnd`. **No single
fixture can assert "the window was cut because this packet was lost".** The
split is: arithmetic in-crate against `Solo`/`Pair`; behaviour in `tests/`
against `FlakyWire`; and the brief must not ask either to do the other's job.

**F5 — `Solo`'s core under test is always the responder** (`tests_streams.rs:
3438–3441`). Nothing in §12–§14 is role-dependent, so this is not expected to
bite; recorded because it bit slice 4a's watermark tests and the debts in §6.4
are exactly those tests.

**F6 — duplication is capped at 2 copies** (`testutil/mod.rs:696–700`) and
reordering is emergent from `jitter` only. S12 asks the transfer to survive
"loss, reordering and duplication"; both are expressible, neither is tunable to
an adversarial degree. Adequate for S12; recorded so nobody reads a green S12
as an adversarial result.

**F7 — no fixture can express "this send failed" *inside* a specific packet's
transmission, or "this driver stopped"** (`driver.rs:1032–1036` says so). Two
of four seam-review findings were unreachable from all 451 tests for exactly
this reason. Slice 5 adds a new class of driver-side state (three timers, a
map that must be dropped at teardown); a `drop_state` that forgets to clear the
sent map is invisible to every test that does not assert `bytes_in_flight`
after death. **Mitigation:** R-19 plus an explicit teardown assertion.

**F8 — the mutation obligation.** Every one of R-10, R-11, R-12, R-13, R-18
asserts a numeric outcome that a *plausibly wrong* controller also produces on
a *healthy* path. Working rule 9 says a name is not a pin; ruling 114 adds that
mutation found what reading did not, and that the first mutation tried
(`STREAM_FILL_QUANTUM`) survived. **The 5a integrator must mutate at least: the
`<=` in the admission gate to `<`; `push_back` to `push_front` in the loss
re-queue; `while` to `if` in congestion avoidance; the persistent-congestion
period from `pto_count = 0` to the backed-off PTO; and the recovery-period test
from `<=` to `<`.** Each must red exactly one named test and green on
restoration. Working rule 10: commit before mutating.

**F9 — 664 tests stand at `d39ed9b`** (488 unit + 176 integration, plus 7
doc-tests). Ruling 113's `fin` argument touches ~24 call sites in files two
prior test authors own. Doing it after dispatch, or in the same commit as
anything else, is how a mutation revert eats unrelated work (working rule 10,
which cost the maintainer two accessors). It is listed as the integrator's
pre-dispatch job in §3 for that reason.

---
## APPENDIX R — raw research notes (planner's working record, kept for audit)

### R.1 §12/§13/§14 extract (SPEC.md:3553–3919), read in full

**§12.1** ACK = descending ranges, RFC 9000 §19.3.1 in varints. First block
`largest − first_range ..= largest`; each `(gap, range)` pair: block_largest =
`prev_smallest − gap − 2`, covers `block_largest − range ..= block_largest`.
Descending below zero = structural failure (§8.4). `ack_delay` = **raw
microseconds varint, no exponent**. No ECN.

**§12.2** ACK derived from the replay window snapshot (greatest + bitmap, §7.2)
— "the single received-packet record; there is no second tracker". Ranges
emitted **newest-first, descending**, truncated at `MAX_ACK_RANGES` **or**
packet capacity, whichever binds. `MAX_ACK_RANGES = 64` = cap on
`range_count` = the (gap,range) pairs; ≤65 blocks including the first.
Received ACK with `range_count > 64` = malformed, §8.2 structural class
(nothing applied; CLOSE `PROTOCOL_VIOLATION`).

**§12.3** `ack_delay` measured from arrival of the packet bearing `largest` to
emission of the ACK, µs. If window's largest was **not frame-seen** (keepalive
counter, §7.5) → `ack_delay = 0`. RTT estimator subtracts peer's ack_delay
capped at `MAX_ACK_DELAY`.

**§12.4** `MAX_ACK_DELAY = 25 ms`. ACK owed after every **2nd** ack-eliciting
packet, or when `AckDelay` timer fires (armed at MAX_ACK_DELAY on receipt of
the **first unacknowledged** ack-eliciting packet), whichever first. ACK owed
**immediately** on out-of-order arrival (counter not exactly one greater than
previous greatest — opens/fills/sits in a gap); first ack-eliciting packet of
a session applies vacuously → immediate ACK. Owed ACK rides next outgoing
packet (§8.5); else standalone. Pure-ACK packets: `seal_quiet`, not
ack-eliciting, never tracked for loss, bypass cwnd.

**§12.5** Bounded intersecting processing: intersect with in-flight set, never
materialise ranges. O(in-flight × range_count). ACK whose `largest` exceeds
highest counter sealed → **ignored whole**, trace, other frames still apply
(deliberate divergence from RFC 9000 §13.1). Newly acked packets clear frames
from in-flight, feed §13.2 and §14.2. Duplicate ack = no-op.

**§13** "Per connection (one session, §7.8), over the **ack-eliciting**
sent-packet map."

**§13.1** keeps `latest_rtt`, `smoothed_rtt`, `rttvar`, `min_rtt`. Sample when
`largest` newly acked AND ≥1 newly acked packet is ack-eliciting. First:
`srtt = latest`, `rttvar = latest/2`, `min_rtt = latest`. Later:
`min_rtt = min(min_rtt, latest)`; peer ack_delay capped at MAX_ACK_DELAY
subtracted **only when doing so does not push the sample below min_rtt**;
`rttvar = ¾rttvar + ¼|srtt − adjusted|`; `srtt = ⅞srtt + ⅛adjusted`.
Pre-sample seed: `K_INITIAL_RTT = 333 ms`, `rttvar = K_INITIAL_RTT/2`.
`K_GRANULARITY = 1 ms`. Estimator is **connection-scoped**, survives roam as a
**prior**; `min_rtt` re-seeded from first post-roam sample (MUST be allowed to
rise).

**§13.2** Lost when a later packet in its space is acked AND
(`K_PACKET_THRESHOLD = 3` or more counters below largest acked
OR sent > `loss_delay = max(9/8·max(srtt, latest_rtt), K_GRANULARITY)` before
the ack arrived). "Survivors inside the threshold arm the `Loss` timer at
`time_sent + loss_delay` (minimum across in-flight packets)." Lost frames
re-queue by §8.7 class; lost bytes leave `bytes_in_flight`; loss feeds CC
**once per episode** (§14.3).

**§13.3** `PTO = srtt + max(4·rttvar, K_GRANULARITY) + MAX_ACK_DELAY`, anchored
at **the last ack-eliciting send**, doubled per consecutive unanswered probe
(`2^pto_count`), capped `PTO_BACKOFF_CAP = 2^6`. `pto_count` resets to 0
whenever **any packet is newly acknowledged**. Probe train ended by
`DEAD_TIMEOUT` (ruling 33: first probe arms the death deadline, no later probe
re-arms). **Pto armed only while ≥1 ack-eliciting packet is in the sent map**
(RFC 9002 §6.2.1); disarmed when map empties; **Loss timer takes precedence**
(§16.5).

**§13.4** Firing PTO sends **one** ack-eliciting packet: pending
retransmittable frames **oldest-first** if any, else a **bare PING**.
Deliberate simplification of RFC 9002 §6.2.4. Sealed `seal_quiet` AND exempt
from the cwnd admission gate — two independent properties. **Not** exempt from
§7.3 anti-amplification budget.

**§13.5** Frames, never packets. Sent map holds, per ack-eliciting counter:
**send time**, **the frame identities aboard** (stream ranges, credit frame
identities, RESET_STREAM, PING, DATAGRAM markers), and the packet's **size in
bytes** feeding `bytes_in_flight`. **Non-ack-eliciting packets are never
inserted.**

**§13.6** Roam is the only recovery seam. Sent map **kept**. CC resets with
**pre-roam flight fenced off**: pre-roam packets resolve for loss and
retransmission but feed **no congestion event, no persistent-congestion walk,
no RTT sample, no app_limited window growth**. RTT estimator suspect-but-kept,
`min_rtt` re-seeded. Consequence: `bytes_in_flight` may exceed the fresh
window; gate blocks new sends for ≤1 loss/PTO cycle, kept probeable.

**§14.1** `trait Controller { on_sent(&mut, now, bytes: u64);
on_ack(&mut, now, sent_time, bytes: u64, app_limited: bool);
on_congestion_event(&mut, now, sent_time, is_persistent: bool, lost_bytes: u64);
window(&self) -> u64 }`. NewReno the one v1 impl. Nothing CC-related on the wire.

**§14.2** `INITIAL_WINDOW = 12000`, `MINIMUM_WINDOW = 2400`,
`LOSS_REDUCTION_FACTOR = 0.5`, `ssthresh` starts `u64::MAX`. Slow start
(cwnd < ssthresh): cwnd += bytes newly acked. Congestion avoidance: integer
ABC — accumulate acked bytes, add one `MAX_DATAGRAM` each time the accumulator
exceeds cwnd. No floating point.

**§14.3** On congestion event (any loss of an ack-eliciting packet):
`cwnd = max(cwnd·0.5, MINIMUM_WINDOW)`, `ssthresh = cwnd`, recovery period
starts at the event. Events for packets **sent before** recovery start are
ignored. **Symmetrically, acks of packets sent before recovery start do not
grow the window** — the same `sent_time ≤ recovery_start` test gates both.
`on_congestion_event` fires **once per episode, after the full lost-packet
scan**, never once per lost packet.

**§14.4** Persistent congestion computed **inside the §13.2 walk**: two
ack-eliciting packets sent more than
`persistent_period = PTO × PERSISTENT_CONGESTION_THRESHOLD` apart, both lost,
**no packet acked between them**, **and a prior RTT sample exists** →
`cwnd = MINIMUM_WINDOW`, slow start effectively restarts. `persistent_period`
uses the §13.3 formula **with `pto_count = 0`** (backoff excluded).
Pre-roam packets excluded from the walk. `PERSISTENT_CONGESTION_THRESHOLD = 3`.

**§14.5** `bytes_in_flight` = Σ `size` over the sent map (ack-eliciting only).
Gate: `bytes_in_flight + candidate_size ≤ cwnd`. **Exemptions, exhaustively**:
PTO probes; the contested-connection probe (§7.5) — exempt from *admission
only*, still tracked and still counted in flight (ruling 43); non-ack-eliciting
control packets (pure ACKs, CLOSE, keepalives) — never tracked, never gated.
DATAGRAM frames **are** congestion-controlled (count in flight, gated) but are
never retransmitted. **Exemptions are cwnd-scoped only** — §7.3's
anti-amplification budget binds all output including the exempt classes.
`app_limited`: set when the sender runs out of queued data with cwnd headroom
remaining, **recorded onto each sent packet**; `on_ack` reads the acked
packet's recorded flag; when set, no window growth on that ack.

**§14.6** Controller resets to initial state (cwnd = INITIAL_WINDOW,
ssthresh = u64::MAX) on **roam only**. **The recovery-period marker is set to
the roam instant — not cleared.** Pre-roam packets fenced (no congestion event,
no persistent-congestion walk, no RTT sample, no app_limited growth) — RFC 9000
§9.4 per-path separation, "quinn's path-generation stamping". Stated divergence
from RFC 9000 §9.4: RTT estimator kept as a prior.

**§14.7** No pacing, no ECN, no CUBIC/BBR.

### R.2 Supporting spec sections (verified by targeted read)

- **§7.2** (SPEC.md:1859–1891): `REPLAY_WINDOW` = 2048 bits, `[u64;32]`.
  Greatest + bitmap. Check-then-mark strictly post-AEAD. **ACK stays fused**
  (ratified 2026/08/14): ACK fidelity capped at 2048 counters; an ACK-loss
  burst longer than the window's time-width yields delivered-but-unreported
  packets → spurious retransmissions (streams dedup by offset §9.5) and at
  worst one spurious congestion event absorbed by §14.3's recovery period.
- **§8.3** (2637–2653): ACK is type `0x02`, **not ack-eliciting**, retransmission
  class **never**. Fields: `largest, ack_delay, range_count, first_range,
  (gap, range)*`. PING `0x01` is ack-eliciting, class never.
- **§8.4** (2666–2676): ACK layout verbatim; structural errors: `range_count >
  MAX_ACK_RANGES (64)`, any range descending below zero. Semantic no-op
  (ignored whole, traced): `largest` above the highest counter sealed.
- **§8.5** (2825–2836): packing order — **ACK first (if owed)**, then control
  frames (credit, RESET_STREAM, CLOSE), then STREAM/DATAGRAM fill, **then PING
  last if a probe still owes ack-eliciting content**. ≤1 extends-to-end frame,
  final position. STREAM fill is round-robin, one quantum per stream per pass.
- **§8.6** (2838–2843): every slither seal ≤ `MAX_PLAINTEXT` + 16 = 1186 B.
- **§8.7** (2845–2870): ack-eliciting iff ≥1 ack-eliciting frame. Only
  ack-eliciting packets enter the sent map. Three classes: **ranges** (STREAM —
  only still-un-ACKed sub-ranges resent, split/merge/coalesce freely);
  **regenerate** (MAX_DATA, MAX_STREAM_DATA, MAX_STREAMS_*, RESET_STREAM — the
  *identity* re-queues and carries the **freshest current value**; RESET_STREAM
  re-emits until acked or state discarded, with the §9.6/§9.8 receiver-overflow
  reset carve-out retained at connection level); **never** (PADDING, PING, ACK,
  DATAGRAM, CLOSE).
- **§10.6** (3402–3480): credit **is** the buffer commitment. "no non-stream,
  non-datagram frame can force unbounded buffering — **ACK processing is
  bounded intersecting (§12.5)**". `REASSEMBLY_CHUNKS_MAX` = 1024.
- **§15.2** (3940–3970): local close drops **all stream, flow-control, recovery
  and congestion state immediately** — "data unacknowledged at `close()` is
  never retransmitted and data still queued behind the congestion window is
  never sent at all; ruling 47's `acked()` is how an application waits for
  delivery **before** closing, and this clause is deliberately unchanged by
  it". Receiving CLOSE: hold a `CLOSE_LINGER` drain, **then** drop all state.
  CLOSE is not ack-eliciting and is never retransmitted by loss detection.
- **§16.2** (4360–4419): `SendStream::acked()` resolves once every byte written
  to that stream **and its FIN** are acknowledged — the `StreamFinished` event
  awaited; legal and expected **after** `finish()`, never returns
  `WriteError::Finished` for that reason; `Reset(code)` if reset before its
  data was acked; `ConnectionLost` if the connection died first.
  `Connection::acked()` takes a **snapshot** — every byte handed to the
  connection at the instant of the call, across every stream, including the
  §9.8 message streams — resolving when each is acknowledged **or abandoned by
  a reset**. Bytes written after the call do not extend it.
- **§16.4** (4772–4818): `core::Connection`'s verb list and `ConnOutput` /
  `ConnEvent` / `ToEndpoint::Retired { our_index: u32 }`.
  `ConnEvent::StreamFinished { r }` = "send half fully acknowledged".
- **§16.5** (4961–5021): named timers **`Keepalive`, `PersistentKeepalive`,
  `Liveness`, `Loss`, `Pto`, `AckDelay`, `CloseLinger`, `Contested`**.
  `handle_timeout` idempotent — each due timer stopped before its logic runs;
  "**For `Loss`/`Pto` the idempotency additionally rests on synchronous
  sealing (§16.7)**". **Equal-deadline priority list is normative and
  exhaustive**: per connection — *loss detection beats PTO and **exactly one
  of the two fires per evaluation***; teardown collection (Liveness,
  CloseLinger, Contested) precedes keepalive evaluation; **`AckDelay` fires
  after the loss/PTO evaluation at the same instant** (the owed ACK then rides
  any probe or retransmission that evaluation produced, §8.5);
  `PersistentKeepalive` evaluated last. Lateness bound `L` = 250 ms is a
  **shell** conformance parameter, not the protocol's.
- **§7.4 quiet set** (quoted in ruling 98, SPEC.md:1953–1958): `seal` marks
  `last_send` for packets carrying **≥1 first-transmission STREAM frame or
  DATAGRAM frame**, and the keepalive. `seal_quiet` = pure ACKs, **PTO probes,
  retransmissions**, the four credit frames, RESET_STREAM, and CLOSE.

### R.3 Rulings digest (grep-targeted, not read whole)

- **47 / 54** (rulings.md:862–890, SPEC §16.2): `acked()` not `flush()`; two
  verbs, no more. `StreamFinished` is the core event both read.
- **80** (1523–1546): `now` on every mutating call; the cores never invent an
  instant. Generalisation: *an API listing looks exhaustive and literal and is
  routinely neither.*
- **81** (1559–1600): `Closed(_)` at the death; **`Retired` follows the
  `CloseLinger` expiry on the closing and draining paths**, and the `Closed`
  event within the same drain only on paths with **no post-mortem** (liveness,
  nonce exhaustion, `Replaced`, teardown before a session exists).
- **98** (2247–2286): §7.4 already enumerates the quiet set. Table: **STREAM
  first transmission → `seal`; STREAM retransmission → `seal_quiet`**;
  RESET_STREAM → `seal_quiet`; all four credit frames → `seal_quiet`; **every
  one of them ack-eliciting**. *Marking is a property of the seal, not the
  frame* — a packet mixing a fresh STREAM frame with credit frames is marking.
- **105** (2416–2427): the tombstone obligation's **loss-driven variant** —
  "free a stream, drop the ACK, let the peer's PTO retransmission re-name it".
- **108** (2509–2543): `read`, `finish`, `abandon_recv` take `now`. *General
  form: in a sans-io core whose sealing is synchronous, `now` belongs on every
  verb that can emit a frame.*
- **111** (2580–2595): RESET_STREAM `final_size` = end offset of the highest
  byte actually **transmitted**. The window between the two readings is
  *exactly the accepted-but-unsealed set*.
- **113** (2607–2638): `on_ack_range`/`on_lost_range` are `pub(crate)` verbs on
  `Connection` taking `now`. Debts listed.
- **114** (2690–2750): §16.7 bounds **where** a seal happens, not **how much**
  is sealed. In slice 4, a mutating call flushes everything the ledger admits;
  *"the seam is one call site: `pump()` gains a second bound"*.
  Third misreading recorded in ruling 129: **114 bounds where sealing happens,
  not when transmission happens.**
- **118 as amended by 128** (2923–2941, 3181–3250): see §6.1 below.
- **124** (3031–3080): total precedence for every 4b verb — (1) the handle's
  own terminal state, (2) the connection's death latch, (3) the empty-buffer
  short-circuit, (4) the core call. `closed_locally` is a three-state
  `LocalEnd`, not a `bool`.
- **125** (3085–3110): a stream handle's `Drop` performs the last-handle
  `close(NO_ERROR, "")`.
- **126** (3115–…): `Cargo.toml` `[[test]]` stanzas belong to the integrator
  (working rule 15) — committed **commented out** by the implementer.
- **129** (3252–3275): §9.6's no-op is correct; the reachable positive control
  is owed to slice 5 because only a congestion window creates
  sealed-but-unsent/accepted-but-unsealed data.

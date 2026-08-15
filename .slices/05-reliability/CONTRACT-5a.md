# CONTRACT-5a — the binding API contract for slice 5a (core reliability)

**Status: BINDING.** All three 5a agents compile against this and none may
change it. If you believe something here is wrong, **say so in your report
and implement it as written anyway** (working rule 5).

Derived from `.slices/05-reliability/PLAN-5.md` §2, §4 and §5, with every
open question resolved by **Round 23 (rulings 130–140)** in
`.spec-v2-clean-slate/rulings.md`.

---

## 0. The decisions Round 23 took — read these first

They override anything in the extracted sections below that predates them.

| # | Decision |
|---|---|
| **130** | **§13 and §14 are slice 5's**, not slice 7's. Six texts said otherwise and all descended from a slice-4 planning doc. Two debts move here with it: **ruling 105's loss-driven tombstone variant** and **ruling 98's STREAM-retransmission seal row** (§12 does not force it; **§13 does**). |
| **131** | §13.2's `Loss` timer arms from the **minimum across the survivors below `largest_acked`**, never across the whole map. Packets above `largest_acked` are not judged by that walk. |
| **132** | Ruling 128's "guard (b)" was misdescribed and **ruling 81 is not touched**. On the draining path `drop_state` runs at `CloseLinger` expiry, not at the death. The real blockers are ruling 118's accept latch, ruling 124's read precedence, and `core::Connection::read`'s `self.lost` guard. |
| **133** | **Local close frees `streams`/`flow` at once. The draining (peer-CLOSE) path retains them for `CLOSE_LINGER`. The no-linger deaths retain nothing** — and that consequence is documented, not discovered. |
| **134** | **`write()` accepts bytes the congestion window cannot yet send.** The window defers the *seal*, never the acceptance. `write()` stays a flow-control verb and the window is invisible to it. |
| **135** | **Both `acked()` verbs answer from their settled snapshot before the death latch** — ruling 128's defect on the sender's side. |
| **136** | A packet's `size` is the **full datagram**: `DATA_HEADER_LEN + ciphertext + AEAD_TAG_LEN` = `Transmit::data.len()`. A window derived in datagram units is spent in datagram units. |
| **137** | **`SentPacket` carries a `u32` path generation from slice 5**, held at 0. §14.6's single recovery marker cannot serve §13.6's four fences: it is also set by every ordinary congestion event, so reusing it for the RTT fence suppresses sampling after every loss episode. |
| **138** | §13.1's *"and at least one newly acknowledged packet is ack-eliciting"* is **vacuous and MUST NOT be implemented** — §13.5 never inserts non-ack-eliciting packets. Do not build the tracking to evaluate it. Note "newly acknowledged" *is* load-bearing in the first clause. |
| **139** | `pto_count` increments **at the `Pto` timer's firing**. Persistent congestion **does not clear** `recovery_start`. `app_limited` is stamped on **the packet that emptied the queue with headroom left**. `Controller` is **`pub(crate)`**, deliberately. `SendStream::acked()` before `finish()` **parks** (rustdoc the hazard). §13.2's time threshold is **`>`**, not `>=` — test it one-sidedly on both sides. |
| **140** | Doc comments must not cite `.slices/**/PLAN.md` section numbers as though they were `SPEC.md`'s. Already corrected in `send.rs`, `streams.rs`, `mod.rs`, `tests_streams.rs`. |

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


---

## Appendix — what 5a must NOT build

`acked()` (both verbs) and ruling 128's post-death drain are **5b's**, and
5b runs *after* 5a. 5a defines and unit-tests the core entry points they
need; it wires nothing to the shell.

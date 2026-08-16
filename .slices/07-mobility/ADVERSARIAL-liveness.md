# Adversarial review — liveness, timers, queues, resource exhaustion

Lens: §7.5, §9, §10, §11, §12, §13, §14, §15. Reviewer: adversarial (liveness).
Status: COMPLETE. 7 findings, 2 ranked HIGH.

Out of scope by instruction: ruling 203's sizing defect; the deferred
STOP_SENDING round itself (ruling 204) — its consequences are in scope.

## 0. Base / provenance

## 1. Findings (ranked)

### F1 — **The driver livelocks whenever a keepalive or beacon is owed but not admissible.** Ruling 141's defect, surviving in §7.5's timers. **Severity: HIGH. Confidence: high (code-path certain; reachability argued below).**

**The defect.** `Connection::transmit_keepalive` (`src/core/connection/mod.rs:2215`)
refuses to seal when either of two conditions holds (2223):

```rust
if self.contested.is_pending() || !self.amplification.admits(size) { return; }
```

It returns **without moving `last_send`**. Its caller
`transmit_keepalive_if_owed` (2183) then calls `sync_liveness_timer()`
unconditionally (2201), and `sync_liveness_timer` (2142) re-arms

- `TimerKind::Keepalive` at `last_send + KEEPALIVE_TIMEOUT` (2157) whenever
  `owes_passive_keepalive()`, and
- `TimerKind::PersistentKeepalive` at `last_send + interval` (2165) whenever the
  beacon is configured,

neither of which consults `contested` or `amplification`. Since `last_send` did
not move, **both deadlines are re-armed at an instant that has already passed**.

`Timers::next()` (`timers.rs:171`) is a plain `min`, `Connection::poll_output`
hands it out as `ConnOutput::Timeout`, and `Driver::deadline`
(`src/shell/driver.rs:915-938`) takes the `min` **with no clamp to `now`**.
`Driver::run` step 4 then does `sleep_until(deadline)` (258) on a past instant,
which completes immediately, calls `handle_timeout` (283), whose `take_due(now)`
finds the same timer due, which calls `transmit_keepalive_if_owed` again, which
re-arms it in the past again. **The driver spins at 100 % CPU.**

This is *exactly* ruling 141, quoted from `rulings.md:3585`:

> the re-arm goes through `sync_recovery_timers`, so `poll_output` announces a
> deadline at or before `now`, the shell schedules an immediate wake, and **the
> pair spins** … a failure no functional test sees, because every byte still
> arrives.

Ruling 141 swept `sync_recovery_timers`. `sync_liveness_timer` has the same
shape and was not swept. `read()`'s own doc comment (mod.rs:766-771) names
"ruling 141's spin, from a new direction" as a thing to avoid — so the hazard
was known and the keepalive path was still missed.

**Concrete sequence (the beacon form — the one §7.4 itself describes).**
§7.4 (SPEC.md:2336-2350) states the case in terms: a replayed initiation
produces a half-open session at an address that will never validate, and *"the
session emits at most 588 B for the replayed 196 B and then **goes quiet** until
the address validates … here 'until' means 'never'"*.

1. Responder accepts a replayed msg1. `Amplification::arm(floor, INIT_PACKET_LEN)`
   (`mod.rs:1748`) ⇒ `recv = 196`, budget `3 × 196 = 588`.
2. The RESP packet costs 107 ⇒ `sent = 107`, 481 B left.
3. The application configured a persistent keepalive at 1 s (admissible: the band
   is `[1 s, 25 s)`).
4. Beacons of 30 B fire at t=1 … t=16. `107 + 16×30 = 587 ≤ 588`. The 17th needs
   617 > 588 ⇒ **refused**.
5. From t = 17 s the beacon timer is re-armed at `last_send + 1 s = t17 ≤ now`
   on every evaluation. The driver spins until the liveness deadline at
   install + 25 s reaps it — **≈ 8 s of hot loop**, and see F2: every spin
   iteration re-drains *every* connection on the endpoint.

The connection does not "go quiet"; it goes quiet **on the wire** and hot in the
CPU. §7.4's sentence describes the wire and is silent about the timer, which is
working rule 8's shape — a stated construction (*"then goes quiet"*) whose scope
(the wire only) is unstated.

**The unbounded form.** The bounded case above is capped by the liveness
deadline, which is armed because beacons are *marking* sends. Two variants have
**no such cap**:

- **`contested.is_pending()`.** The guard at 2223 blocks the keepalive
  unconditionally, budget or no budget. A pending mark clears only on an ACK
  covering the floor (`on_ack_coverage`, 1284) or when the budget admits the
  probe — both of which require the peer to send. A peer that provokes the mark
  and then goes silent leaves the mark pending; if `owes_passive_keepalive()` is
  set, the `Keepalive` timer spins.
- **`armed == false`.** `owes_passive_keepalive()` is set by a receive, which
  also sets `armed = false` (`session.rs:336-342`). A pure ACK is *neither*
  marking *nor* ack-eliciting (§7.4), so it does not re-arm. In that state
  `Liveness::deadline()` returns `None` — **the death clock is disarmed** — and
  nothing bounds the spin at all. It ends only when the peer sends or the
  application closes.

**Cost.** 100 % of one core, per affected connection, plus F2's O(N) fan-out.
Nothing on the wire; no test sees it (ruling 141's own observation); no metric.

**No test covers it.** `grep -rn "spin\|livelock"` over the test tree finds only
`tests_recovery.rs:703-855` (ruling 141's guards) and `spec_streams.rs`'s
caller-spin tests. Nothing in `tests_roam.rs`/`tests_contested.rs` exercises a
budget-refused or mark-blocked keepalive.

**Suggested shape of the fix (not a ruling).** The two refusal points are
"held, not dropped" in §7.3's language, so the timer must be re-armed at *the
instant the hold could lift*, not at the instant that already passed — or the
shell must clamp `deadline` to `>= now`, which is a second-line defence for the
whole class rather than for this instance. Which one is right is a ruling, not
an implementation choice; §16.5's *"an armed deadline `D` fires no earlier than
`D`"* does not say what a core may announce.

---

### F2 — **Every timer event dirties every connection: an idle endpoint is quadratic in connection count, and the self-sustaining dance guarantees the events.** Severity: HIGH. Confidence: high.

**The defect.** `Driver::handle_timeout` (`src/shell/driver.rs:886-896`):

```rust
for record in self.conns.values() {
    if let Some(core) = cell.core.as_mut() { core.handle_timeout(now); }
    cell.dirty = true;                       // <-- unconditional
}
```

One expired deadline anywhere on the endpoint calls `handle_timeout` on **every**
connection core and marks **every** connection dirty, whether or not it had a
timer due. `serve()` (318) then drains all N, and `deadline()` (915) polls all N.
So the per-event cost is O(N), not O(1) — and it is O(N) *full core drains*, not
O(N) cheap checks.

**Why this is a liveness finding and not merely a performance note.** Target 1's
premise is the mechanism: §7.5's dance is self-sustaining and unconditional
(ruling 39 — *"automatic for any connection that has carried traffic"*). Every
connection that has ever carried one byte therefore produces **at least one
timer event every `KEEPALIVE_TIMEOUT`, forever, with no application involvement
and no way to stop it short of closing**. That is N events per 10 s, each
costing O(N) ⇒ **O(N²/10) core drains per second**, on an endpoint doing nothing.

At N = 1 000 that is 10⁵ drains/s; at N = 10 000 it is 10⁷ drains/s. The
`serve()` loop additionally rescans `self.conns` to *collect* the dirty set
(331-336) on every one of its up-to-`DRAIN_BOUND` passes.

**What the attacker gets (target 1's question, answered).** The handshake is IK,
so the attacker needs a static the application accepts — this is an
authenticated-peer attack, not an anonymous one. Given one accepted identity:

- open N connections, send **one byte** on each, then answer only the keepalives;
- cost to the attacker: 0.1 packet/s per connection;
- cost to us: 0.1 packet/s per connection **plus** the quadratic driver term
  above, **plus** per-connection state held forever;
- **there is no idle reaper.** §7.6 is deleted, liveness is the only reaper
  (`session.rs:288`), and the dance is designed to defeat it. §17.5 rules
  established connections *"application-governed — unbounded by the protocol"*,
  so the only bound is the application's accept policy — and the application has
  no signal distinguishing "idle but alive" from "parked by an adversary",
  because §7.5 makes them the same wire behaviour.

**A legitimate silent-but-alive peer costs exactly the same**, which is the
point: the protocol cannot tell them apart, so any mitigation is necessarily an
application policy. What the *protocol* can fix is the quadratic term, which is
purely a shell defect and has no wire consequence.

**Fix shape.** `handle_timeout` should consult the core's announced deadline
before calling into it, and dirty only cores it actually called — the deadline
is already tracked per connection to compute the `min`. Also fixes half of F1's
blast radius.

---

### F3 — **`Reassembly::insert` costs O(span of the stored chunk), and a peer can re-trigger it with a free 1-byte duplicate.** Severity: MEDIUM-HIGH. Confidence: high on the code path; the multiplier depends on how much the application has left unread.

**The defect.** `RecvHalf::apply_stream` (`src/core/connection/recv.rs:243-262`)
inserts **every** STREAM frame that passed `check_stream`. `check_stream` (198)
rejects only on final-size violations and on `high_water.max(end) > advertised`,
so a frame lying **wholly inside already-received offset space** passes, and
charges `delta = 0` at the connection level (259-261).

`Reassembly::insert` (467) then:
- skips only bytes below `read_offset` — i.e. only what the application has
  already **read** (476-483);
- computes the merge span `lo..hi` over every overlapping *or adjacent* chunk;
- allocates `vec![0u8; (stop - start) as usize]` (515) and copies the whole span
  back into it (517-523);
- removes the merged chunks with `VecDeque::remove` in a loop (524-526), which is
  O(n) each.

The cost of one arriving frame is therefore **O(the span of the chunk it
touches)**, and the peer chooses both the span and the arrival rate.

**Concrete sequence.**
1. Peer opens a stream and sends offsets `1 .. 262 144` contiguously, **omitting
   byte 0**. That is one stored chunk of 262 143 B at offset 1. `read_offset`
   stays 0 forever, so the application can never drain it and never will
   (`Reassembly::read`, 545-551, returns 0 unless `front.offset == at`).
2. Peer then sends a **1-byte STREAM frame at offset 5**, in a fresh packet
   (authenticated, window-fresh, new counter — not a replay).
3. `check_stream` passes (`end = 6 ≤ high_water`), `delta = 0` — **no flow
   control is consumed**. `insert` merges: one 256 KiB `alloc_zeroed`, one
   1-byte copy, one 256 KiB copy, one 256 KiB free.
4. Repeat at line rate.

**Cost.** ≈ 512 KiB of memory traffic plus a 256 KiB allocate/free cycle per
~40-byte datagram — on the order of **10⁴× CPU amplification**, sustainable
indefinitely, invisible to flow control, invisible to the replay window, and not
counted anywhere. At 1 000 packet/s that is ~500 MB/s of memcpy and 1 000
256 KiB allocations per second, per connection.

**Why the application cannot escape it in-band.** The reader has no
STOP_SENDING (§19, ruling 204). Its only lever is dropping the `RecvStream`,
which calls `abandon_recv` (`src/shell/stream.rs:868`) — so the escape exists,
but it costs the stream, and nothing tells the application that this is what is
happening. The `slither::frames` traces do not cover it.

**Note on §12.7's neighbour.** `apply_reset` (recv.rs:275-288) discards the
buffer when the reset is *applied*, with the comment *"holding the buffer until
observation would let a peer pin 1 MiB behind an application that never reads"*.
The pinning hazard is understood; the **re-coalescing** hazard on the same
buffer is not addressed anywhere I could find.

---

### F4 — **§17.5's *"the credit term dominates"* is arithmetically false; reassembly metadata is ~10× the credit term.** Severity: MEDIUM. Confidence: medium-high (allocator-dependent constant, not the shape).

§17.5 (SPEC.md:6174) bounds an established connection at *"the advertised credit
— ≤ `INITIAL_MAX_DATA` (1 MiB) plus per-stream book-keeping and reassembly
metadata bounded by `REASSEMBLY_CHUNKS_MAX` (§10.6 — **the second bound is what
makes the credit term the dominant term rather than a 25–50× underestimate**)"*.

The bound is per **stream**, not per connection (`Reassembly` is a field of
`RecvHalf`), and `REASSEMBLY_CHUNKS_MAX` bounds the **count**, not the cost of a
count. Reachable worst case:

| quantity | value |
|---|---|
| peer-opened streams | 32 bidi + 128 uni = **160** (§10.2) |
| chunks per stream | **1 024** (recv.rs:538) |
| chunks total | **163 840** |
| minimum offset span for 1 024 disjoint non-adjacent chunks | 2 047 B |
| connection credit consumed | 160 × 2 047 = **327 520 B** — well inside 1 MiB |
| `VecDeque<Chunk>` storage | 163 840 × 32 B = **5.2 MB** |
| heap allocation per 1-byte `Vec<u8>` | 16–32 B (allocator minimum) ⇒ **2.6–5.2 MB** |
| **total metadata** | **≈ 8–11 MB** |

So the credit term (1 MiB advertised, 320 KB actually charged) is **not**
dominant: the metadata is roughly ten times it. `REASSEMBLY_CHUNKS_MAX` does not
"make the credit term dominant" — it changes the multiplier from the "25–50×"
the parenthetical names to ~10×, which is the same defect at a smaller constant.

This is working rule 11's shape at the specification level: the rationale names a
mechanism (the chunk bound) and asserts a consequence (credit dominates) that the
mechanism does not deliver. Ruling 94 already fought one instance of this —
*"allocate lazily … this is the line that would turn 128 peer-opened streams into
32 MiB"* (recv.rs:441) — and fixed the `with_capacity`, not the per-chunk term.

**I am reporting, not resolving.** Whether the right answer is a byte-denominated
reassembly bound, a smaller `REASSEMBLY_CHUNKS_MAX`, or an amended §17.5 row is a
ruling.

---

### F5 — **`warn!` per evicted datagram is a peer-driven log flood.** Severity: MEDIUM. Confidence: high.

`Datagrams::push_recv` → `trace_drop` (`src/core/connection/datagram.rs:147-154`)
emits a `tracing::warn!` for **every** eviction, and the doc comment states the
level is deliberate: *"a default subscriber sits at `INFO`, so a lower level
would leave the drop silent for exactly the operator §11.5 is written for."*

`DATAGRAM_RECV_QUEUE` is 64. A peer that sends datagrams the application does not
claim produces **one warn-level record per datagram past the 64th**, at the
peer's chosen rate, unbounded, per connection. Each record carries the cumulative
counter, so it is not deduplicated by any structured-logging backend that keys on
the message.

§11.5's requirement is *"a silent drop is a known operability weakness … and is
deliberately not copied"* — a requirement about **visibility**, with the **rate**
unstated (working rule 8 again). The two are separable: a counter plus a
rate-limited or exponential-backoff record satisfies §11.5's stated purpose
without handing the peer a log-write amplifier. Note that a datagram costs the
peer ~31 B on the wire and costs us a formatted structured log record — the
amplification is in bytes-written-to-disk, not packets.

Same argument applies to `push_send`, but that one is application-driven and so
is not an attack surface.

---

### F6 — **`Reassembly::read` is O(bytes remaining) per call; a small-buffer reader against a large chunk is quadratic.** Severity: LOW-MEDIUM. Confidence: high.

`Reassembly::read` (`recv.rs:545-560`) serves from the front chunk with
`front.data.drain(..n)`, which memmoves the remainder of the chunk down by `n`
on every call. The peer chooses the chunk size (up to `INITIAL_MAX_STREAM_DATA`,
256 KiB, by sending contiguously); the application chooses `n` (its `read` buffer,
which for an `AsyncRead` adapter is commonly 4 KiB or less, and for a
byte-oriented parser can be much smaller).

A 256 KiB chunk drained in 64-byte reads costs ≈ 4 096 × 128 KiB ≈ **512 MB of
memmove for 256 KiB delivered** — a ~2 000× amplification that the peer sets up
for free by sending one large contiguous run. It is not an unbounded attack (it
is bounded by delivered bytes × chunk size / read size) and a well-behaved
application with a large buffer never sees it, which is why this is ranked below
F3.

---

### F7 — **The pending-mark / keepalive interaction has a second, quieter consequence than F1's spin.** Severity: LOW. Confidence: medium.

`transmit_keepalive` refuses while `contested.is_pending()` (2223), citing ruling
171's priority order. That is right for the budget's sake, but it means a
connection with a pending mark **stops answering §7.5's passive dance entirely**
for as long as the mark is pending. The peer, receiving nothing, reaches its own
`DEAD_TIMEOUT` and dies at `R + 25 s` — a plain liveness death on a connection
that was healthy and that the contested machinery was never meant to kill (§15.4
is explicit that the contested row's peer view is *"a healthy peer is unaffected
and keeps its side for its own `DEAD_TIMEOUT`"*).

Whether the pending gap may swallow the keepalive for up to 25 s, given §15.4's
promise about the healthy peer, is a question for a ruling. I am flagging it as a
**conflict between two statements**, per working rule 3, and not resolving it:
§7.3/ruling 171's priority order says the pending probe outranks the keepalive;
§15.4's contested row says a healthy peer is unaffected. They differ exactly when
the mark stays pending longer than `DEAD_TIMEOUT − KEEPALIVE_TIMEOUT`.


## 2. Sub-areas with no findings

Stated explicitly, per the brief, rather than padded into findings.

- **Nonce exhaustion (target 6) — not drivable.** §7.9's space is
  `0 ..= 2⁶⁴ − 2` per direction. The peer's only lever on our seal rate is
  eliciting ACKs, and §12.4 caps that at one ACK per two ack-eliciting packets
  (`ack.rs:120`), so driving us to the bound needs ~3.7 × 10¹⁹ packets. On the
  receive side `MAX_EPOCH_JUMP` = 2 (`constants.rs:156`, pinned to hiss by a
  `const` assertion at 589) caps a forward counter jump at 2 × 65 536 per packet,
  so a peer cannot make us skip counter space either. `NonceExhausted` is
  correctly wired at both seal sites (`mod.rs:2232`, `mod.rs:2374`). **No
  finding.**

- **Ruling 195's equal-`Instant` class (target 2) — swept, one residual, no
  defect found.** Every `Instant` comparison I could find in the connection core
  is inclusive on the side that matters:
  - `Timers::due`: `deadline <= now` (`timers.rs:186`) — the firing instant fires.
  - `Liveness::deadline`: `R + DEAD_TIMEOUT`, fired at the deadline (ruling 85).
  - `transmit_keepalive_if_owed`: `last_send() >= now` (`mod.rs:2187`) — a marking
    send **at this instant** correctly suppresses the keepalive; `>` would have
    reproduced ruling 195 one level up.
  - `Closing::reply`: `>= CLOSE_REPLY_MIN_INTERVAL` (`close.rs:121`).
  - `Recovery`: `>= loss_delay` (`recovery.rs:515`) — ruling 141's `>=`.
  - `NewReno::in_recovery`: `sent_time <= recovery_start` (`congestion.rs:95`) —
    RFC 9002's inclusive form; a packet sent *at* the recovery start does not
    trigger a second reduction.
  - `Amplification::on_ack_covering` and the contested clear: `largest >= floor`
    (`mobility.rs:151`, `mod.rs:1273`/`1284`) — inclusive, which is what makes
    "the counter the next seal will use" a satisfiable floor rather than an
    off-by-one that can never be cleared.
  - `Recovery` persistent congestion: `sent_at.duration_since(first) > period`
    (`recovery.rs:636`) — strict, and correctly so: equal instants mean zero
    duration, which is not persistent congestion.
  The only comparison that *is* a bare `R > S` in spirit is gone — ruling 195
  replaced it with `received_since_marking_send` (`session.rs:277`) and
  `sync_liveness_timer` uses the flag (`mod.rs:2154`). **No new instance found.**

- **Bounded queues at the bound (target 3) — clean, except F5's trace.**
  `Datagrams::push` (`datagram.rs:128-136`) is `len() >= bound` ⇒ evict exactly
  one ⇒ push, so the 65th datagram evicts the 1st and the queue is never 65 deep;
  `unpop_send` (94) can only re-insert what `pop_send` just took within one
  synchronous core call, so it cannot overshoot. `MAX_ACK_RANGES` is enforced on
  both sides — truncation newest-first on emit (`ack.rs:202`, with the correct
  *"the dropped oldest ranges are the ones prior ACKs most likely already
  carried"* discipline) and a hard parse rejection at 65 on receipt
  (`frame.rs:546`), with both sides of the boundary tested (`frame.rs:1241-1243`).
  `CLOSE_REASON_MAX` is const-asserted to fit one packet (`constants.rs:569`).
  Ruling 161's "the packing pass never evicts" boundary is clean: `pop_send`/
  `unpop_send` are the packing pass's only queue verbs and neither can drop.

- **Flow control and credit (target 4) — no unrecoverable exhaustion found.**
  A peer that opens streams and never reads them harms only itself: peer-opened
  and locally-opened stream indices are separate spaces
  (`Flow::local_max_streams` / `remote_max_streams`, `flow.rs:170-174`), so
  holding all 32 bidi + 128 uni open blocks the peer's own opens and not ours.
  A peer that pins connection credit with unreadable data (the offset-0 gap of
  F3) pins ≤ `INITIAL_MAX_DATA` = 1 MiB, and the reader **does** have a recovery
  path despite having no STOP_SENDING: dropping the `RecvStream` calls
  `abandon_recv` (`src/shell/stream.rs:868` → `mod.rs:802`), whose ruling-93
  true-up brings the connection contribution to `credit.advertised()`
  (`recv.rs:334-339`) and releases the credit. The escape is real but
  **undiscoverable from the API alone** — nothing signals that this is the
  situation — which is the deferred round's cost, as the brief anticipated.
  The credit term itself is not exhaustible in a way that does not recover.

- **Close and linger (target 5) — ruling 133's data-loss concern is closed by
  ruling 128, and I could not reopen it.** `die()` → `drop_state()`
  (`mod.rs:1678-1720`) deliberately does **not** drop `streams` and `flow`, and
  `Connection::read` (772-787) serves buffered bytes after the death and reports
  `ConnectionLost` only when nothing is left (781). So the sequence I went
  looking for — sender's `acked()` returns `Ok(())`, path breaks, receiver dies
  at `DEAD_TIMEOUT` with fully-received bytes still unread — does **not** lose
  the data: the reader still drains it. The retention has no timer on the
  no-linger paths (`disarm_all()` at 1696), so the bytes live until the handle is
  dropped. The `CLOSE_LINGER` paths bound it at 5 s, which is spec'd. **No
  finding.**

- **CLOSE reply amplification.** ≤ 1 reply/s for 5 s, each ≤ ~290 B against a
  ≥ 31 B trigger, and the trigger must be **authenticated and window-fresh**
  (`close.rs:112-118`), so a replay cannot drive it. ~9× against the genuine peer
  only, five times. Not worth a finding.

- **`REPLAY_WINDOW` (2048).** A peer can jump `greatest` forward by at most
  `2 × REKEY_EPOCH_MSGS` per packet (hiss's `MAX_EPOCH_JUMP`), so it cannot blank
  the window with one packet from an arbitrary counter; and the window is a
  receive-side structure whose only client is the ACK derivation, which is
  O(32 words). **No finding.**

## 3. Notes / raw evidence

### 0.1 Base

- Main tree (not a worktree). `git log --oneline -1` = `ebc1663 Rulings 204-207: API surface ratified, and ruling 203's fix brief`.
- Uncommitted: only the three `.slices/07-mobility/ADVERSARIAL-*.md` files (mine and two peers'). Source tree is clean, so reads of `src/` reflect the commit.

## 3. Notes / raw evidence

### 3.1 §7.4/§7.5 as read (SPEC.md:2268-2871)

- Marking = `seal` (fresh app STREAM/DATAGRAM, keepalive). Quiet = everything
  else, by *characterisation* (ruling 190), list illustrative.
- Death: `now - last_authenticated_recv >= DEAD_TIMEOUT` AND >=1 arming send
  since that recv. Arming = marking OR ack-eliciting. Armed at install.
- Passive keepalive: `R > S` (received since last marking send) AND
  `now - S >= KEEPALIVE_TIMEOUT` -> send keepalive (marking).
- Dance is self-sustaining after one exchange (ruling 39, explicitly intended).
- Beacon: opt-in, [1s, DEAD_TIMEOUT), default 10s, marking, exempt from cwnd,
  bound by §7.3 amplification budget.
- Contested probe: one mark per connection, floor = next_counter, deadline
  KEEPALIVE_TIMEOUT armed at PING *transmission*; pending if budget blocks.
  Ruling 175: real probe rate is min(refusal rate, 1/RTT), ~100/s on LAN.
  Declared a *cost* defect, "no cooldown is added", deliberately.

### 3.2 The keepalive/beacon re-arm path (raw)

`src/core/connection/mod.rs:2183-2251`

```
fn transmit_keepalive_if_owed(&mut self, now, passive) {
    let Some(liveness) = self.liveness().copied() else { return };
    if liveness.last_send() >= now { return }
    if !passive || liveness.owes_passive_keepalive() { self.transmit_keepalive(now); }
    self.sync_liveness_timer();      // <-- unconditional re-derive
}
fn transmit_keepalive(&mut self, now) {
    ...
    if self.contested.is_pending() || !self.amplification.admits(size) { return; }   // 2223
    ... seal (moves last_send) ...
}
```

`sync_liveness_timer` (2142) arms `Keepalive` at `last_send + KEEPALIVE_TIMEOUT`
whenever `owes_passive_keepalive()`, and `PersistentKeepalive` at
`last_send + interval` whenever the beacon is configured. Neither consults
`contested` or `amplification`.

`src/shell/driver.rs:915-938` `deadline()` = `min` over cores, **no clamp to
now**; `run()` step 4 does `sleep_until(deadline)` (258) →
`tokio::time::sleep_until` on a past instant completes immediately.

### 3.3 Reassembly (raw)

`src/core/connection/recv.rs:243-262` `apply_stream` calls
`reassembly.insert(offset, data, read_offset)` for **every** STREAM frame whose
`check_stream` passed — and `check_stream` (198) only rejects on final-size and
on `high_water.max(end) > advertised`. A frame **wholly inside** already-received
offset space passes both and charges `delta = 0` connection credit (259).

`insert` (467-542):
- `offset < read_offset` bytes are skipped (476) — only the already-*read* prefix.
- merge span `lo..hi` is every chunk overlapping or adjacent;
- `let mut merged = vec![0u8; (stop - start) as usize];` (515) — a fresh
  allocation spanning the **whole** merged range, then the arriving bytes are
  copied in and then every old chunk is copied over it (517-523);
- `for _ in lo..hi { self.chunks.remove(lo); }` (524-526) — `VecDeque::remove`
  is O(n) each.

So the cost of one arriving frame is O(span of the chunk it touches), not
O(len of the frame), and the peer picks both.

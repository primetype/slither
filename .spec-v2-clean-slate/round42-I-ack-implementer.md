# IMPL-ACK-REPORT — ruling 271, ACK cadence: every-2nd → per-drain coalescing

Base commit (rule 14, verified first act): `99998d57c17cccfec73b79ed2fee90a8ea1212cd`
(`99998d5 Round 42: the residual profiled — it moved into the syscall bucket`) — matches brief.

## Conflicts / questions for the record

Five, ordered by what ruling 271 has to decide.

### C1. A measured regression I am reporting rather than hiding: 100 ms RTT at **default** windows, −8 %

`sweep-rtt`'s default-window row goes **1.47 → 1.36 MiB/s**, reproducibly
(1.36 / 1.35 / 1.36 across three runs; the baseline's own min/max was
1.45/1.47). The other four RTT rows are +3 %, +12 %, +15 % and flat, and every
rtt = 0 row is +22 % to +34 %.

Two controls, run rather than reasoned about:

- **`ACK_COALESCE_MAX = 2`** makes `pending` unreachable and collapses the code
  to the pre-271 emission point. It returns **1.46 MiB/s and the 1-per-2
  cadence** — the baseline, reproduced through the *new* code path. **The
  refactor is therefore clean; the regression belongs to the coalescing.**
- **`ACK_COALESCE_MAX = 8`** gives **1.36** — identical to 32, at a cadence of
  1-per-7.6 versus 1-per-21. **The regression is depth-insensitive and
  saturates immediately.**

I could not identify the mechanism and refuse to invent one (working rule 11).
Three candidates were opened and refuted against the code — byte-based slow
start, `app_limited` (which moves the wrong way), and §12.3/§13.1's
`ack_delay` subtraction. **This is ruling 271's call: an 8 % loss in the
slowest configuration against +25–34 % everywhere else.**

### C2. `ACK_COALESCE_MAX` is a new constant that the ratified direction did not name

The brief ratified per-drain coalescing with *"every other ratified
constant/timer untouched"*. It did not mention a bound, and pure per-drain
coalescing needs none for **correctness** — a peer obeying a congestion window
must eventually stop and wait, so the drain always ends. But it is
self-limiting *by way of a sender stall*, which is a throughput hazard rather
than a safety property. I added the valve (32, additive, `src/constants.rs`)
and I am flagging it rather than presenting it as implied.

**And it is currently the binding constraint**, which is why the bench lands
at 108–111 MiB/s rather than the brief's ≈115: the measured cadence is 28.3
data-per-ack against a valve of 32, so drains are *longer* than the valve.
Raising it to 64 or removing it should close the remaining 4–6 % and move the
ACK fraction toward quinn's 1.68 %. I did not ship that because a bound I can
defend beat 4 % I cannot. **Removing it is a one-line change with a known
direction and an unmeasured downside; measuring 64 is the obvious next
experiment.**

### C3. The driver's `select!` arm order is now protocol, and no document says so

Per-drain coalescing works because `Driver::run` step 4 is `biased` with
`recv_from` **above** `sleep_until`. Reorder those two arms and the cadence
silently becomes **one ACK per received datagram** — worse than the policy 271
replaces — with all 766 tests green and nothing on the wire to show for it but
a datagram census. This is working rule 8's shape (a stated construction with
an unstated scope). I put the note in `src/shell/driver.rs`; §16.5 needs the
ratified sentence, drafted at *Drafted spec text* (c). **`SPEC.md` is not
modified — the sentence is a draft, not a fact.**

### C4. One red is a harness limitation, not a superseded assertion

`smoke::a_snapshot_settles_only_once_its_bytes_are_acknowledged` fails because
the `exchange` helper (mod.rs:3911) loops *deliver → drain* until quiet and
**never calls `handle_timeout`**. Under coalescing the burst's last ACK is
released only by `AckDelay`, so the loop goes quiet with an ACK pending. This
is working rule 13 — the fixture bounds the coverage: an in-core harness
written as "deliver until quiet" models a network and not a driver. It is the
integrator's file and its call; I list it, I did not fix it. The same shape
will bite anyone writing a new core-level test against the new cadence.

### C5. `is_owed()` split into two questions, and every old caller kept the narrow one

`is_owed()` now means *"an ACK is owed **and the pump must build a packet for
it**"*; the new `is_ready()` means *"pack it into a packet already being
built"*. All four pre-271 call sites — ruling 217's `PATH_CHALLENGE` `offer`
disjunct, ruling 203's `transmit_pure_ack` refusal path, the pump's loop exit,
and `pack_ack` — were re-decided individually rather than swept. Three keep
`is_owed()`; only `pack_ack` moves to `is_ready()`. If ruling 271 disagrees
with any single one of those four, the change is local — but the choice was
deliberate and is argued at each site, not defaulted.

## Mechanism found

### Where the every-2nd counter lives

`src/core/connection/ack.rs` — `AckState::since_ack`, a `u64` incremented once
per **authenticated, window-fresh, ack-eliciting** packet in
`AckState::on_recv` (ack.rs:112). The trigger:

```rust
let in_order = prev_greatest.is_some_and(|g| g.checked_add(1) == Some(counter));
if !in_order || self.since_ack >= 2 {
    self.owed = true;
    return AckAction::Now;
}
AckAction::Arm(now + constants::MAX_ACK_DELAY)
```

`since_ack` is reset **only** in `on_ack_packed()` (ack.rs:137), called from
`Connection::pump_packets` (mod.rs:2636) and `transmit_pure_ack` (mod.rs:2916).
`owed` is a sticky flag; `pack_ack` (mod.rs:2861) reads it.

Call chain per received datagram:

`Driver::run` step 4 `select!` → `Event::Received` → `Driver::handle_datagram`
(driver.rs:914) → `Connection::handle_datagram` (mod.rs:556) →
`fold_ack_policy` (mod.rs:1320) → `AckState::on_recv`, then `apply_live`
(mod.rs:1676) → `Connection::pump(now)` (mod.rs:1688 / 1856).

### Why the cadence is 1-per-2 *in practice*

**Confirmed by reading `Driver::run` (driver.rs:213–312): the driver delivers
exactly ONE datagram per loop iteration.** Step 4's `tokio::select!` has a
single `wire.recv_from(&mut buf)` arm; the arm calls `handle_datagram` for that
one datagram and falls back to the top of the loop, where step 1 `serve()`
drains both cores to `Timeout`. There is no socket-level batching anywhere.

So on a bulk receiver every delivered data packet is followed by a pump:

| delivered pkt | `since_ack` | `on_recv` | pump result |
|---|---|---|---|
| 1 | 1 | `Arm(now+MAX_ACK_DELAY)` | nothing owed → no packet |
| 2 | 2 | `Now` (owed, disarm) | `transmit_pure_ack` → **1 ACK datagram** |
| 3 | 1 | `Arm` | — |
| 4 | 2 | `Now` | **1 ACK datagram** |

⇒ exactly 1 ACK-only datagram per 2 data datagrams = 33.3 % of wire traffic,
which is round42-G's measured 33.6 %.

**The `owed` flag is already per-pump-coalescing.** It is a boolean, not a
queue: N ack-eliciting packets delivered before a single pump produce **one**
ACK frame covering all N (`ack::derive` reads the whole replay window). The
every-2nd counter is not what forces one ACK per two packets — *the driver's
one-datagram-per-pump loop is*. `since_ack >= 2` only decides whether the ACK
waits for `MAX_ACK_DELAY` or goes out on the very next pump.

**This is the finding that picks the design.** A purely core-side change
(shape (b)) cannot help: if the driver pumps once per delivered datagram, then
"one ACK per pump" *is* "one ACK per datagram" — 67 % ACK traffic, strictly
worse than today. The lever is in the shell.

## Design

### The shape chosen: (b′) core-side, drain-boundary via a due-immediately `AckDelay`

**Not (a).** Batching the driver's socket drain was evaluated and rejected as
*insufficient on its own and more invasive*: the core pumps **inside**
`Connection::handle_datagram` (`apply_live` → `pump`), so the driver could
deliver 32 datagrams in one turn and the core would still have emitted 16
ACK-only packets on the way through. Making batching work needs the core
change anyway — and then the batching itself buys only driver-loop turns
(round 42-H buckets I+J ≈ 0.80 µs) while bending §16.4's every-call-drain
letter and requiring a `poll`-once-and-drop of an application-supplied
`Wire::recv_from`. Rejected: strictly more risk for the smaller half of the win.

**What was implemented.** §12.4's *every-2nd* trigger stops **emitting** and
starts **arming**. Three states instead of two:

| trigger | before | after |
|---|---|---|
| 1st unacked ack-eliciting pkt | `Arm(now + MAX_ACK_DELAY)` | **unchanged** |
| out-of-order (gap) | owed **now**, emits in this receive | **unchanged** |
| 2nd (and later) ack-eliciting | owed **now**, emits in this receive | ACK **pending**; `AckDelay` armed **at `now`** (due immediately) |
| `AckDelay` fires | owed now | **unchanged** |
| ≥ `ACK_COALESCE_MAX` pending | — | promoted to owed now (safety valve) |

`AckState` gains one field, `pending`. The two states differ in exactly one
way:

- **owed** — an ACK must go out; the pump *builds a packet for it* if nothing
  else is pending. (`is_owed()`; all four existing call sites keep this
  meaning, including ruling 217's `offer` disjunct and ruling 203's
  `transmit_pure_ack` refusal path.)
- **pending** — an ACK is due and **rides** the next outgoing packet (§12.4
  verbatim), but does **not** build one of its own.

The pump enforces the second half with one new guard, after every packing
stage has run:

```rust
// [ruling 271] A *pending* ACK rides a packet; it never builds one.
if ack_packed && !self.ack.is_owed() && packing.frames().len() == 1 {
    break;
}
```

### Why arming at `now` *is* the drain boundary

The core is sans-io and has no notion of a "drain". The driver's notion of
*"the receive drain has ended"* is precisely *"nothing else is ready right
now"*, and step 4's `select!` is `biased` with `recv_from` **above**
`sleep_until(deadline)` (driver.rs:273–280). So a deadline already due:

- **loses** to every datagram still available on the socket — the burst keeps
  being delivered, `pending` stays pending, `since_ack` keeps climbing, and
  the single ACK that eventually goes out covers all of them (`ack::derive`
  reads the whole replay window; the `owed`/`pending` flags are booleans, not
  a queue);
- **wins** the instant the socket goes empty — `Event::Timeout` →
  `handle_timeout` → `AckDelay` → `on_delay_expired` → `owed` →
  `pump_inner` (mod.rs:723, 753). One ACK, one turn later, microseconds not
  milliseconds.

This is the same mechanism §12.4 already ratifies (*"or when the `AckDelay`
timer fires — whichever first"*). **Only the arming instant changed.** No new
core method, no §16.4 API-list change, no §16.5 change, no `Wire` change, and
`MAX_ACK_DELAY` and `ACK_ELICITING_PER_ACK` keep their ratified values and
their ratified jobs.

### What is preserved, deliberately

- **`MAX_ACK_DELAY` as the outer bound.** An isolated ack-eliciting packet
  followed by silence still arms at `now + MAX_ACK_DELAY` and still flushes
  there — the idle-after-burst tail the brief names. Nothing waits longer than
  it ever did; the coalescing path waits *less*.
- **Immediate-on-gap, byte for byte.** `!in_order` still returns
  `AckAction::Now` and still emits inside the receive, before the drain ends.
  It is checked **before** the coalescing branch. Reordering and loss recovery
  see exactly the pre-271 signal.
- **Ruling 33's liveness-neutrality.** Untouched: pure ACKs are still
  `seal_quiet`, still not ack-eliciting, still not in the sent map, still
  outside the congestion window.
- **Every wire byte.** No frame, header, constant value or layout moved.
  `ACK_ELICITING_PER_ACK` is still 2 and still the trigger.

### The one additive constant: `ACK_COALESCE_MAX`

`src/constants.rs`, **additive**. Without a bound the deferral is limited only
by the sender running out of congestion window — self-limiting, but by way of
a sender stall, which is a throughput hazard rather than a safety property.
`ACK_COALESCE_MAX` promotes `pending` to `owed` once that many ack-eliciting
packets have gone unacknowledged, capping reverse-path silence at 32 data
packets (~38 KB at `MAX_DATAGRAM`) regardless of what the scheduler does. It
also puts a hard floor of 1/32 = 3.1 % under the ACK fraction, which is the
"low single digits" the brief predicts.

**It is not a ratified constant and this report does not claim it as one** —
see *Drafted spec text* for where it would land if ruling 271 wants it, and
*Conflicts* for the alternative (no bound at all) with its measurement.

## §16.4 contract interaction

**§16.4 is not bent, and that was a design goal, not a happy accident.**

The brief's shape (a) — batching the driver's socket drain — *would* have bent
it: `handle_datagram` is a mutating call on the core, and delivering N of them
before one `poll_output()` drain contradicts §16.4's *"every mutating call is
followed by draining `poll_output()` to the terminal `Timeout`"* directly.
That shape was rejected on its own merits (see *Design*), so the question does
not arise.

What the shipped design uses instead is machinery §16.4 and §16.5 already
ratify, unchanged:

| §16.4 / §16.5 obligation | status |
|---|---|
| every mutating call followed by a drain to `Timeout` | **unchanged** — the driver still drains after every `handle_datagram` and every `handle_timeout` |
| nothing deferred to `poll_output()` (§16.7) | **unchanged** — `poll_output` is still a pure pop; the ACK is built inside `handle_timeout`, itself a mutating call |
| `handle_timeout` idempotent, every due deadline stopped before its logic runs (§16.5) | **unchanged** — `take_due` disarms `AckDelay` before `on_delay_expired` runs |
| the core API list (§16.4) | **unchanged** — no method added, removed or re-signed |
| `Wire` (§16.3) | **unchanged** — no `try_recv`, no poll-once-and-drop, no new cancel-safety obligation |

**One thing §16.4/§16.5 do not say, and now should.** The core is sans-io and
has no notion of a "receive drain"; the *driver's* notion of one is *"nothing
else is ready"*, and that notion lives in exactly one place — `Driver::run`
step 4's `biased` `select!`, with `recv_from` above `sleep_until`. Ruling 271
makes that ordering **protocol** rather than a scheduling preference: swap the
two arms and the ACK cadence silently becomes one per received datagram —
worse than the policy 271 replaces — with every test green and nothing on the
wire to show for it but a datagram census. This is working rule 8's shape
exactly (a stated construction with an unstated scope) and it is the one
contract note the design needs. Drafted in *Drafted spec text* (c); the
code-side note is already in `src/shell/driver.rs` at the `select!`.

**A second, smaller one, for the record rather than for the spec.** A
*due-immediately* deadline is now an ordinary steady-state occurrence rather
than the pathology `Driver::deadline`'s `PAST_DEADLINE` assertion hunts. It
does not trip it, and the reason is structural rather than lucky: that
assertion fires only when `last_timeout` is `Some(t)` **and** a core still
announces `<= t` — a timer the firing itself re-armed in the past. `AckDelay`
is armed only in `fold_ack_policy`, which runs only under `handle_datagram`,
and every non-`Timeout` event clears `last_timeout`. Ruling 255's streak-of-3
threshold is untouched. The 24 green integration targets — which ride the real
driver on tokio's paused clock — are the measurement behind this paragraph,
not the argument (working rule 12).

## Existing tests that pin the old cadence

**12 reds, all in the lib target, all pinning the superseded emission point.**
Every one of the 24 integration targets in `tests/` is green, including
`spec_ack_burst`, `spec_constants` (112/112 — wire pins byte-identical) and the
golden-wire vectors (12/12). I touched no test file; the partition held.

Per working rule 12 the honest characterisation is *superseded pin*, not *bug
found*: each asserts, correctly for pre-271, that the emission happens
**inside** the receive.

| # | test (`src/core/connection/…`) | assertion | why it is red |
|---|---|---|---|
| 1 | `tests_ack.rs` `policy::an_ack_is_owed_after_every_second_ack_eliciting_packet:922` | `on_recv(2nd) == AckAction::Now` | now `Arm(now)` — the coalescing arm |
| 2 | `tests_ack.rs` `policy::packing_an_ack_resets_the_every_second_counter_not_only_the_debt:1104` | same | same |
| 3 | `tests_ack.rs` `policy::an_unpacked_debt_survives_further_arrivals:1137` | `a.is_owed()` after the 2nd | the debt is now `is_ready()`; `is_owed()` is the narrower *"build a packet for it"* question. **Re-point at `is_ready()` and the property it names is intact** |
| 4 | `tests_ack.rs` `policy_on_the_wire::one_packet_arms_the_timer_and_the_second_emits_the_ack:1191` | 1 ACK datagram from the 2nd `deliver` | 0 — it is emitted at the drain boundary (`handle_timeout`) |
| 5 | `tests_ack.rs` `policy_on_the_wire::a_replayed_packet_does_not_advance_the_every_second_counter:1348` | as above, on the 2nd *fresh* packet | as above. **The property under test — a replay advances nothing — still holds**: `since_ack` is untouched by replays, only the observation point moved |
| 6 | `tests_ack.rs` `processing::a_padding_only_packet_elicits_no_ack:1865` | 1 ACK on the 2nd eliciting packet | as above |
| 7 | `tests_ack.rs` `processing::a_keepalive_elicits_no_ack_but_is_still_acknowledged:1900` | 1 ACK on the 2nd eliciting packet | as above |
| 8 | `tests_ack.rs` `processing::the_wire_delay_is_zero_when_a_keepalive_holds_the_greatest:1931` | 1 ACK on the 2nd eliciting packet | as above |
| 9 | `tests_ack.rs` `processing::a_full_sixty_four_pair_ack_acknowledges_only_what_is_in_flight:1739` | **fixture precondition**: 400 `deliver`s burn > 130 counters, on *"each ack-eliciting packet the peer sends is answered by a pure ACK"* | burns **13** = 400/32 + 1. **This is `ACK_COALESCE_MAX` working, measured**: the valve fires on exactly every 32nd packet. The fixture needs a `handle_timeout` per burst, or ~4 200 deliveries |
| 10 | `mod.rs` `smoke::the_second_in_order_packet_draws_the_ack_the_first_only_arms_the_timer:4237` | 1 ACK from the 2nd delivery | 0 — drain boundary |
| 11 | `mod.rs` `smoke::an_ack_drains_the_sent_map_and_completes_the_send_half:4169` | `AckDelay == now + MAX_ACK_DELAY` (*"the odd packet out waits on the timer"*) | `now` — the coalescing arm is earlier, and `fold_ack_policy`'s new `min` keeps the earlier of the two. The deadline is 25 ms **sooner**, not later |
| 12 | `mod.rs` `smoke::a_snapshot_settles_only_once_its_bytes_are_acknowledged:4435` | the snapshot settles after `exchange` | **the most interesting red, and it is not an assertion about ACKs at all.** `exchange` (mod.rs:3911) loops *deliver → drain* until both cores go quiet and **never calls `handle_timeout`**. Under coalescing the last ACK of a burst is released only by `AckDelay`, so the loop goes quiet with an ACK pending and the sender never sees the acknowledgement |

**Row 12 is the one worth generalising, and it is working rule 13 exactly —
the fixture bounds the coverage.** Any in-core harness written as *"deliver
until quiet"* models a network but not a driver, and per-drain coalescing is
invisible to it as anything but a lost ACK. The fix is one line at the
quiescence point — fire `handle_timeout(now)` on both cores when
`from_a.is_empty() && from_b.is_empty()`, then loop once more — which is
exactly what the real driver does when its socket empties. It is the
integrator's call; I did not make it.

**And the reason nothing else went red is worth stating, because it is the
strongest evidence in this report:** the `tests/` suites all ride the real
driver over `FlakyWire` on tokio's paused clock, where the due-immediately
`AckDelay` fires exactly as it does over a kernel socket. 24/24 green is the
coalescing being *driven*, not *simulated*.

## Bench

Same machine, same session, `bench_vs_tcp`. The baseline was measured by
restoring the four changed files to `99998d5` with `git checkout --` and
rebuilding; the change was then restored from a scratchpad copy and verified
byte-identical with `diff` before every later run.

### `-- bulk` (rtt = 0)

| windows | baseline | **after** | Δ | ack fraction | data-per-ack |
|---|---|---|---|---|---|
| default | **85.23** MiB/s | **108.20 – 110.55** MiB/s | **+27 % … +30 %** | 33.61 % → **3.42 %** | 1.98 → 28.3 |
| 8Mi/16Mi | **103.73** MiB/s | **129.26 – 133.48** MiB/s | **+25 % … +29 %** | 33.35 % → **3.06 %** | 2.00 → 31.7 |

Two `after` figures are quoted because run-to-run spread is real: raw kernel
TCP in the same two runs moved 11 504 → 11 208 MiB/s (−2.6 %). The lower
figure is the final tree; the higher is the identical *functional* code before
the comment-only doc edits.

```
BASELINE (99998d5)
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default   n=5 p50=85.23  min=84.19  max=85.40  note=amp=1.044,loss=-0.0000,dg_out=1573677,dg_in=1573711,ack_dg=796635,mtu=1181
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi  n=5 p50=103.73 min=103.55 max=106.45 note=amp=1.045,loss=-0.0001,dg_out=1927347,dg_in=1927518,ack_dg=964343,mtu=1188
BENCH scenario=bulk proto=tcp      rtt_ms=0 windows=kernel-default n=5 p50=11504.64 min=11184.37 max=11633.97

AFTER (final tree, the commit)
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default   n=5 p50=108.20 min=107.50 max=108.67 note=amp=1.045,loss=0.0000,dg_out=1996031,dg_in=1996009,ack_dg=70570,mtu=1187
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi  n=5 p50=129.26 min=128.66 max=131.16 note=amp=1.045,loss=0.0000,dg_out=2397160,dg_in=2397160,ack_dg=75588,mtu=1187
BENCH scenario=bulk proto=tcp      rtt_ms=0 windows=kernel-default n=5 p50=11207.87 min=11132.30 max=11359.10

AFTER (first run, identical functional code)
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default   n=5 p50=110.55 min=110.04 max=112.04 note=amp=1.045,loss=0.0000,dg_out=2045657,dg_in=2045656,ack_dg=72315,mtu=1187
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi  n=5 p50=133.48 min=132.92 max=135.86 note=amp=1.045,loss=0.0000,dg_out=2477116,dg_in=2477116,ack_dg=78088,mtu=1187
```

**Against the brief's ≈115 MiB/s target at default windows: 108–111, short by
~4–6 %.** The gap is `ACK_COALESCE_MAX`. At the measured cadence the valve is
*binding* — 28.3 data-per-ack against a valve of 32 — so the drains are longer
than 32 and a larger valve would buy more. See *Conflicts*.

### `-- sweep` (rtt = 0, window ladder)

| windows | baseline | after | Δ | ack_dg / dg_out |
|---|---|---|---|---|
| default | 81.32 | **109.20** | **+34 %** | 758 783/1 498 998 → 71 478/2 021 860 |
| 512Ki/2Mi | 84.37 | **109.90** | **+30 %** | 783 388/1 556 258 → 70 604/2 019 388 |
| 1Mi/4Mi | 90.67 | **110.80** | **+22 %** | 839 830/1 673 453 → 66 605/2 046 661 |
| 2Mi/8Mi | 100.78 | **132.12** | **+31 %** | 932 839/1 862 054 → 78 535/2 447 651 |
| 8Mi/16Mi | 99.86 | **131.31** | **+31 %** | 922 106/1 843 098 → 76 891/2 437 931 |

```
BASELINE
BENCH scenario=sweep proto=slither rtt_ms=0 windows=default   n=5 p50=81.32  min=76.99 max=84.43  note=...,dg_out=1498998,ack_dg=758783,mtu=1179
BENCH scenario=sweep proto=slither rtt_ms=0 windows=512Ki/2Mi n=5 p50=84.37  min=83.49 max=85.13  note=...,dg_out=1556258,ack_dg=783388,mtu=1187
BENCH scenario=sweep proto=slither rtt_ms=0 windows=1Mi/4Mi   n=5 p50=90.67  min=89.28 max=91.67  note=...,dg_out=1673453,ack_dg=839830,mtu=1186
BENCH scenario=sweep proto=slither rtt_ms=0 windows=2Mi/8Mi   n=5 p50=100.78 min=99.33 max=102.48 note=...,dg_out=1862054,ack_dg=932839,mtu=1187
BENCH scenario=sweep proto=slither rtt_ms=0 windows=8Mi/16Mi  n=5 p50=99.86  min=96.65 max=101.99 note=...,dg_out=1843098,ack_dg=922106,mtu=1188

AFTER
BENCH scenario=sweep proto=slither rtt_ms=0 windows=default   n=5 p50=109.20 min=108.35 max=110.56 note=...,dg_out=2021860,ack_dg=71478,mtu=1187
BENCH scenario=sweep proto=slither rtt_ms=0 windows=512Ki/2Mi n=5 p50=109.90 min=108.32 max=110.36 note=...,dg_out=2019388,ack_dg=70604,mtu=1188
BENCH scenario=sweep proto=slither rtt_ms=0 windows=1Mi/4Mi   n=5 p50=110.80 min=110.76 max=111.11 note=...,dg_out=2046661,ack_dg=66605,mtu=1187
BENCH scenario=sweep proto=slither rtt_ms=0 windows=2Mi/8Mi   n=5 p50=132.12 min=131.22 max=133.63 note=...,dg_out=2447651,ack_dg=78535,mtu=1186
BENCH scenario=sweep proto=slither rtt_ms=0 windows=8Mi/16Mi  n=5 p50=131.31 min=130.99 max=133.82 note=...,dg_out=2437931,ack_dg=76891,mtu=1187
```

### `-- sweep-rtt` (rtt = 100 ms) — **the finding, reported and not hidden**

| windows | baseline | valve = 2 (control) | valve = 8 | **valve = 32 (shipped) run 1 / run 2** |
|---|---|---|---|---|
| **default** | **1.47** | **1.46** | 1.36 | **1.36 / 1.35 — −8 %** |
| 512Ki/2Mi | 2.52 | 2.62 | 2.60 | 2.57 / 2.60 — +3 % |
| 1Mi/4Mi | 4.86 | 4.68 | 4.74 | 5.16 / 5.45 — **+12 %** |
| 2Mi/8Mi | 9.19 | 9.08 | 9.59 | 10.69 / 10.45 — **+15 %** |
| 8Mi/16Mi | 17.31 | 17.23 | 18.35 | 16.92 / 17.30 — flat |

**The 100 ms default-window column regresses ~8 %, reproducibly: 1.36, 1.35,
1.36 across three runs against a baseline of 1.47 whose own min/max was
1.45/1.47. Four of five rows improve or are flat; that one does not, and it is
the slowest, most latency-sensitive configuration in the suite.**

Two controls were run rather than reasoned about:

1. **`ACK_COALESCE_MAX = 2`, which makes `pending` unreachable** — `since_ack
   >= 2` then satisfies both thresholds on the same packet, collapsing the
   code to the pre-271 emission point. Result: **1.46 MiB/s and `ack_dg/dg_out`
   back to 1-per-2** — the baseline, reproduced *through the new code path*.
   **This isolates the regression to the coalescing itself and clears the
   refactor** (the `is_ready`/`is_owed` split, the `min` arming rule, the
   pump's break) of any unintended side effect.
2. **`ACK_COALESCE_MAX = 8`.** Result: **1.36 MiB/s — identical to 32.** The
   regression is **depth-insensitive**: at valve 8 the cadence is 1-per-7.6 and
   at valve 32 it is 1-per-21, and throughput is the same to two decimals. So
   it is not *"we coalesced too hard"* — it is the drain-boundary deferral
   itself, and it saturates immediately. Since depth does not move the
   regression but moves bulk throughput a great deal (108–111 at 32 against
   103.20 at 8), **32 is the value to ship**.

```
BASELINE
BENCH scenario=sweep-rtt rtt_ms=100 windows=default   n=5 p50=1.47  min=1.45  max=1.47  note=...,dg_out=27268,ack_dg=13819
BENCH scenario=sweep-rtt rtt_ms=100 windows=512Ki/2Mi n=5 p50=2.52  min=2.50  max=2.54  note=...,dg_out=46602,ack_dg=23415
BENCH scenario=sweep-rtt rtt_ms=100 windows=1Mi/4Mi   n=5 p50=4.86  min=4.72  max=4.92  note=...,dg_out=88873,ack_dg=44648
BENCH scenario=sweep-rtt rtt_ms=100 windows=2Mi/8Mi   n=5 p50=9.19  min=9.14  max=9.29  note=...,dg_out=168505,ack_dg=84417
BENCH scenario=sweep-rtt rtt_ms=100 windows=8Mi/16Mi  n=5 p50=17.31 min=16.41 max=18.21 note=...,dg_out=316974,ack_dg=158170

AFTER, valve=32, run 1
BENCH scenario=sweep-rtt rtt_ms=100 windows=default   n=5 p50=1.36  min=1.29  max=1.47  note=...,loss=0.0040,dg_out=25422,ack_dg=1174
BENCH scenario=sweep-rtt rtt_ms=100 windows=512Ki/2Mi n=5 p50=2.57  min=2.44  max=2.80  note=...,dg_out=48405,ack_dg=1960
BENCH scenario=sweep-rtt rtt_ms=100 windows=1Mi/4Mi   n=5 p50=5.16  min=4.50  max=5.58  note=...,dg_out=95481,ack_dg=3584
BENCH scenario=sweep-rtt rtt_ms=100 windows=2Mi/8Mi   n=5 p50=10.69 min=9.80  max=12.14 note=...,dg_out=199128,ack_dg=6654
BENCH scenario=sweep-rtt rtt_ms=100 windows=8Mi/16Mi  n=5 p50=16.92 min=16.12 max=17.71 note=...,dg_out=310624,ack_dg=9991

AFTER, valve=32, run 2
BENCH scenario=sweep-rtt rtt_ms=100 windows=default   n=5 p50=1.35  min=1.28  max=1.58  note=...,loss=0.0000,dg_out=25313,ack_dg=1205
BENCH scenario=sweep-rtt rtt_ms=100 windows=512Ki/2Mi n=5 p50=2.60  min=2.40  max=3.15  note=...,dg_out=49447,ack_dg=1986
BENCH scenario=sweep-rtt rtt_ms=100 windows=1Mi/4Mi   n=5 p50=5.45  min=4.90  max=5.71  note=...,dg_out=98493,ack_dg=3716
BENCH scenario=sweep-rtt rtt_ms=100 windows=2Mi/8Mi   n=5 p50=10.45 min=8.68  max=12.46 note=...,dg_out=194553,ack_dg=6521
BENCH scenario=sweep-rtt rtt_ms=100 windows=8Mi/16Mi  n=5 p50=17.30 min=16.50 max=18.20 note=...,dg_out=316640,ack_dg=10308

CONTROL, valve=8
BENCH scenario=sweep-rtt rtt_ms=100 windows=default   n=5 p50=1.36  min=1.26  max=1.60  note=...,dg_out=25943,ack_dg=3414
BENCH scenario=sweep-rtt rtt_ms=100 windows=512Ki/2Mi n=5 p50=2.60  min=2.47  max=2.83  note=...,dg_out=48184,ack_dg=6237
BENCH scenario=sweep-rtt rtt_ms=100 windows=1Mi/4Mi   n=5 p50=4.74  min=4.55  max=6.19  note=...,dg_out=94400,ack_dg=12020
BENCH scenario=sweep-rtt rtt_ms=100 windows=2Mi/8Mi   n=5 p50=9.59  min=8.97  max=10.72 note=...,dg_out=179386,ack_dg=22601
BENCH scenario=sweep-rtt rtt_ms=100 windows=8Mi/16Mi  n=5 p50=18.35 min=17.47 max=19.18 note=...,dg_out=337385,ack_dg=42259

CONTROL, valve=2 (pending unreachable ⇒ pre-271 emission point)
BENCH scenario=sweep-rtt rtt_ms=100 windows=default   n=5 p50=1.46  min=1.46  max=1.46  note=...,dg_out=27347,ack_dg=13865
BENCH scenario=sweep-rtt rtt_ms=100 windows=512Ki/2Mi n=5 p50=2.62  min=2.61  max=2.62  note=...,dg_out=49004,ack_dg=24644
BENCH scenario=sweep-rtt rtt_ms=100 windows=1Mi/4Mi   n=5 p50=4.68  min=4.65  max=4.76  note=...,dg_out=87418,ack_dg=43855
BENCH scenario=sweep-rtt rtt_ms=100 windows=2Mi/8Mi   n=5 p50=9.08  min=8.89  max=9.22  note=...,dg_out=167554,ack_dg=83973
BENCH scenario=sweep-rtt rtt_ms=100 windows=8Mi/16Mi  n=5 p50=17.23 min=16.41 max=18.48 note=...,dg_out=317151,ack_dg=158689

CONTROL, valve=8, bulk
BENCH scenario=bulk proto=slither rtt_ms=0 windows=default  n=5 p50=103.20 min=102.64 max=105.09 note=...,dg_out=1917521,ack_dg=251480
BENCH scenario=bulk proto=slither rtt_ms=0 windows=8Mi/16Mi n=5 p50=123.44 min=123.05 max=125.85 note=...,dg_out=2293242,ack_dg=287412
```

**I did not find the mechanism of the 100 ms regression, and I am not going to
guess at one in a report (working rule 11).** Three hypotheses were checked
against the code and **refuted**: NewReno slow start grows by *bytes*
(`congestion.rs:134`), not per ACK, so cadence cannot change the ramp rate;
`app_limited` moves the *wrong* way (coalescing keeps `bytes_in_flight`
higher, so it is stamped **less** often, which would grow the window faster);
and §12.3's `ack_delay` is still measured to the emission and still subtracted
per §13.1, so the RTT estimate is not inflated. Loss is not it either — the
reproduction at `loss=0.0000` matches the one at `loss=0.0040`.

## Drafted spec text

**Report-only. Nothing below is applied; I did not modify `SPEC.md`.** Three
amendments and the rule-4 sweep.

### (a) §12.4 — the amendment. Insertion point: SPEC.md:4621–4629

Current text, quoted with the table row above it for placement:

```
| Constant | Value |
|---|---|
| `MAX_ACK_DELAY` | 25 ms |

- An ACK is owed after every **2nd** ack-eliciting packet, or when the
  `AckDelay` timer (armed at `MAX_ACK_DELAY` on receipt of the first
  unacknowledged ack-eliciting packet) fires — whichever first.
- An ACK is owed **immediately** on out-of-order arrival: an ack-eliciting
```

Proposed replacement for the table and the **first bullet only** — the second
bullet (immediate on gap) is untouched, and that is load-bearing:

```
| Constant | Value |
|---|---|
| `MAX_ACK_DELAY` | 25 ms |
| `ACK_COALESCE_MAX` | 32 |

- **[AMENDED 2026/08/18 — ruling 271]** An ACK becomes **due** after every
  **2nd** ack-eliciting packet. It is **emitted** at the earliest of: any
  outgoing packet built for another reason, which it rides (third bullet
  below); the end of the receiver's current **receive drain**;
  `ACK_COALESCE_MAX` unacknowledged ack-eliciting packets; or the `AckDelay`
  timer, armed at `MAX_ACK_DELAY` on receipt of the first unacknowledged
  ack-eliciting packet.

  A **receive drain** is one pass of the shell's event loop over everything
  the substrate has already delivered; it ends when no datagram is ready
  (§16.5). A sans-io core cannot observe it and does not have to — the drain
  boundary reaches the core as an `AckDelay` armed at `now`, which by §16.5 is
  due at `now` and therefore fires on the first pass with nothing else ready.

  This replaces *"an ACK is owed after every 2nd ack-eliciting packet …
  whichever first"*, under which the ACK was built inside the receive that
  crossed the threshold. Because the driver delivers one datagram per loop
  turn, that put **one ACK-only datagram on the wire for every two data
  datagrams** — 33.6 % of all wire traffic, measured, against quinn's 1.68 %
  for the same workload, and ≈3.69 µs of a 14.11 µs per-data-datagram budget
  on both sides combined (`round42-G`, `round42-H`). Neither `MAX_ACK_DELAY`
  nor `ACK_ELICITING_PER_ACK` changes value or job; only the emission point
  moved.
```

### (b) §12.4's third bullet — one clarifying clause (working rule 4a)

The *"An owed ACK rides the next outgoing packet"* bullet is **already correct
and must stay**, but under 271 it carries weight it did not carry before: it
is now the rule that stops a coalescing ACK ever costing a piggyback.
Suggested addition, in place:

```
  (**[ruling 271]** This is now the *first* of §12.4's emission triggers
  rather than a convenience: a due ACK riding a packet that already exists is
  the whole reason coalescing does not delay a bidirectional flow.)
```

### (c) §16.5 — the driver-ordering note. New paragraph

§16.5 governs timer evaluation order. It needs the one sentence §12.4's drain
boundary depends on, and working rule 8 is why it must be written down rather
than left as an unstated scope:

```
**[RATIFIED 2026/08/18 — ruling 271]** The shell's event loop MUST poll
inbound datagrams **before** an expired deadline. A deadline already due when
it is read therefore fires on the first pass on which no datagram is ready,
which is what makes it expressible as *"the end of the receive drain"*
(§12.4) and the only reason §12.4's coalescing needs no core API. Polling the
deadline first is not a correctness failure and produces no wire change a
conformance test can see; it silently returns the ACK cadence to one per
received datagram, which is **worse** than the every-2nd policy ruling 271
replaced.
```

### (d) Rule-4 sweep — every SPEC/src site stating every-2nd

From `grep -rn 'every 2nd\|every-2nd\|2nd ack-eliciting\|ACK_ELICITING_PER_ACK'`
over `SPEC.md src examples benches README.md CHANGELOG.md`, excluding test
files:

| site | text | verdict |
|---|---|---|
| `SPEC.md:4624` | §12.4 first bullet | **amend** — draft (a) |
| `SPEC.md:7496` | Appendix conformance list, *"Delayed-ACK policy timing: every-2nd, the 25 ms timer, immediate on gap (§12.4)"* | **must change.** Suggested: *"Delayed-ACK policy timing: every-2nd due, per-drain coalescing bounded by `ACK_COALESCE_MAX`, the 25 ms timer, immediate on gap (§12.4)"* |
| `SPEC.md:7966` | Named-constants table, *"ACK policy \| every 2nd ack-eliciting, `MAX_ACK_DELAY` cap, immediate on gap"* | **must change.** Suggested: *"every 2nd ack-eliciting due, coalesced per receive drain (`ACK_COALESCE_MAX` 32), `MAX_ACK_DELAY` cap, immediate on gap"*, plus a new `ACK_COALESCE_MAX \| 32 \| §12.4` row |
| `SPEC.md:8003` | Unnamed-constant table, `ACK_ELICITING_PER_ACK \| 2 \| §12.4 \| "every 2nd ack-eliciting"` | **stays** — value and quote both still accurate. `ACK_COALESCE_MAX` belongs in the **named** table at 7966, since 271 names it |
| `SPEC.md:8015` | the *"a ratio or rate stated in prose"* lesson, listing `every 2nd` | **stays, and gains a member.** `ACK_COALESCE_MAX` is the same shape and was invented by this implementation, not by the spec — exactly the defect class that passage names |
| `src/constants.rs:403` | `ACK_ELICITING_PER_ACK` doc | **done** — rewritten in place, quoting the pre-271 wording and why it changed |
| `src/constants.rs:424` | `ACK_COALESCE_MAX` | **new** |
| `src/constants.rs:743` | `const _: () = assert!(ACK_ELICITING_PER_ACK <= ACK_COALESCE_MAX)` | **new** — a valve below the trigger makes `pending` unreachable and restores the old cadence silently, which is precisely the control run above |
| `src/core/connection/ack.rs` module doc | *"four scalars"* | **done** — five, with 271's measurement quoted and the new section heading |
| `src/core/connection/ack.rs:38` | *"a duplicate that advanced §12.4's every-2nd counter … free reverse-path amplification"* | **stays, strengthened** — the guard is worth *more* under coalescing, since a burst of genuine packets now buys one ACK |
| `src/core/connection/ack.rs` `on_recv` doc | *"read `prev_greatest` after the mark and the only trigger left is every-2nd"* | **strengthened.** Pre-271 that hazard cost the gap ACK's promptness; it now costs the gap signal **entirely**, because the surviving trigger defers to the drain boundary |
| `src/core/connection/mod.rs:588` | the same `prev_greatest` warning at the call site | **left as written** — accurate; the strengthened version is at the definition |
| `src/core/connection/mod.rs:1313` | the replay/every-2nd amplification note | **strengthened**, same reason as ack.rs:38 |
| `src/core/connection/tests_sizing.rs:806` | *"`ACK_ELICITING_PER_ACK` is 2, so nothing is …"* | **test file — not touched.** Green. Listed so the integrator knows it exists |
| `src/shell/driver.rs` step 4 `select!` | had no note at all | **new** — the arm ordering is now protocol; see (c) |

## Gates

Run in this worktree on the committed tree. Working rule 7: command and
output.

```
$ cargo fmt --all --check
fmt-exit=0

$ cargo clippy --all-features --all-targets -- -D warnings
    Checking slither v0.2.0 (…/agent-a23131b0c4c561823)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.36s     (zero warnings)

$ cargo build --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.94s

$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   Generated …/target/doc/slither/index.html
$ RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
   Generated …/target/doc/slither/index.html

$ cargo +1.96 check --all-features --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.39s

$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok

$ cargo test --all-features --no-fail-fast
  --lib          FAILED. 754 passed; 12 failed        <- exactly the 12 listed above
  tests/* (24)   ok. 0 failed in every target
  spec_constants ok. 112 passed        (wire pins, byte-identical)

$ cargo test --all-features --lib golden
  ok. 12 passed; 0 failed              (golden-wire vectors)

$ cargo test --no-fail-fast                        (default features)
  --lib          FAILED. 754 passed; 12 failed        <- the same 12
  everything else ok

$ cargo test --release --all-features --no-fail-fast
  --lib          FAILED. 754 passed; 12 failed        <- the same 12
  everything else ok
```

**Expected reds: exactly the 12 named above. Nothing outside that list is red
in any of the three test profiles, and the debug and release profiles produce
an identical set — so the `debug_assert!`s added to the pump's coalescing break
do not fire.** Wire pins (`spec_constants`, golden vectors) untouched-green.

Nothing on the release-gate table was skipped.


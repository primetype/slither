# I — Keepalive / death-clock: measurement of the parked-forever construction

Base commit: `94dab200a172a5b62b4d28711962574d61a9d3c8` (verified `git rev-parse HEAD`, tree clean).
Worktree: `/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-adb13e14fdf82c5cd`

## 0. Base verification

```
$ pwd
/Users/nicolasdiprima/work/primetype/slither/.claude/worktrees/agent-adb13e14fdf82c5cd
$ git rev-parse HEAD
94dab200a172a5b62b4d28711962574d61a9d3c8
$ git status --short
(empty)
$ git branch --show-current
worktree-agent-adb13e14fdf82c5cd
```
Base matches the brief. No reset needed.

## 1. The traced construction (from C-keepalive-gate.md §4.2 / §4.2.1)

C's claim, in one line: `Liveness::deadline()` is `None` unless `armed`
(`session.rs:309-312`), `armed` is cleared by the very authenticated fresh
receive that sets the passive-keepalive debt (`session.rs:336-344`), and
`keepalive_can_leave()` (`mod.rs:2956`, `!contested.is_pending() &&
amplification.admits(30)`) can veto the one send that would re-arm it. So the
passive keepalive is owed *precisely* in the window where the death clock is
disarmed, and §7.3 can veto it.

C's six steps (§4.2.1), verbatim-summarised:

1. Established connection on a validated address; §7.2 window holding many
   gaps; `bytes_in_flight` non-trivial.
2. Peer roams: one small ack-eliciting authenticated packet of `L` bytes from
   a new source. `commit_roam` (`mod.rs:1332`) re-arms the budget with credit
   `L` -> `room = 3L` (~90). Congestion controller resets with pre-roam
   flight fenced, `bytes_in_flight` kept, so §14.5's gate is closed.
3. That receive sets `armed = false` **and** the passive debt.
4. Pump: owed ACK coalesces with owed `PATH_CHALLENGE` -> ack-eliciting ->
   §14.5's cwnd gate refuses it -> code falls to "a pure ACK is not gated"
   (`mod.rs:2496-2501`).
5. `transmit_pure_ack` emits an ACK **sized to the whole room** (`packing()`
   clamps plaintext to `room - 30`, `ack::derive` truncates at that room) ->
   `room = 0`, nothing armed, nothing marked.
6. `sync_liveness_timer`: `Liveness = None` (`armed == false`),
   `Keepalive = None` (`admits(30)` false), `PersistentKeepalive = None`;
   `Pto = None`; `AckDelay` disarmed; `Loss` drains.

Terminal state claimed: `Timeout(None)`, no timer armed, no peer that can
ever arrive -> neither sends nor dies.

C explicitly marks this **not measured** (§4.2.3) and predicts that if it
fails, the failing link is step 4/5 (the ACK's reachable size against room).


## 2. Fixture reconnaissance

What the construction needed, and what the fixture gives:

| need | fixture lever | file:line |
|---|---|---|
| one core + a raw peer that can seal anything | `Solo` | `src/core/connection/testfix.rs:1078` |
| **validated** starting budget | `Solo::installed_at` (`anchor_from_msg1 = false`) | `testfix.rs:1094` |
| roam from a chosen source | `Solo::deliver_from(now, src, frames)` | `testfix.rs:1150` |
| **gaps** in the replay window | `s.peer.seal(&[])` and *drop the datagram* — burns a counter | `RawPeer::seal`, `testfix.rs:1029` |
| flight to close §14.5's gate | `write_all` + `Connection::send_datagram` | `testfix.rs:823`, `mod.rs:1113` |
| read the state | `conn.liveness()` (`is_armed`, `owes_passive_keepalive`), `conn.amplification_budget()`, `conn.timer(kind)`, `conn.bytes_in_flight()`, `conn.congestion_window()` | `mod.rs:401/439/415/1231/1237` |

Two fixture facts that shaped the construction and are worth recording:

1. **A stream left with data pending defeats the top-up.** With ~250 KB
   queued, the pump plans a full-size packet, §14.5 refuses it, and the
   pump `break`s before the shaping datagram gets a turn. The write must be
   *bounded* (`write_all` of 9 000 B) so the stream drains, then the flight
   is topped to the cwnd with `send_datagram`s sized to the exact headroom.
2. **The roam drain emits nothing.** The ACK is not yet *owed* at the roam
   instant — `AckDelay` is armed instead. The starving pure ACK is emitted
   on the **next driver step**, when `AckDelay` fires. A test that only
   drains at the roam instant sees `Timeout(Some(..))` and concludes the
   state is fine. The driver loop is what exposes it.


## 3. Core-level construction — the test

`src/core/connection/tests_park.rs` (new, temporary). Three functions:

* `roamed_with_a_starved_budget(start)` — C's steps 1-3, plus the one step
  C's list leaves implicit (**1c: shut §14.5's gate**), returning the core
  and the deadline it announced at the roam instant;
* `run_driver(...)` — follows the announced deadlines exactly as
  `Driver::run` does, recording sends, the death, and the liveness/budget
  state after the first step;
* the two tests: `probe_the_construction` (prints everything) and
  `a_starved_passive_keepalive_must_not_park_the_connection_forever` (the
  separating assertion, §8 below), later joined by
  `the_backstop_does_not_reap_a_connection_whose_peer_comes_back`.

One deviation from C's six steps, forced by the fixture and worth naming:
C's step 1 says *"`bytes_in_flight` non-trivial"*, which is not enough —
§14.5's gate is `bytes_in_flight + size <= cwnd`, so what the construction
needs is `cwnd - bytes_in_flight < room`. The fixture drives the headroom
to **exactly 0** (bounded stream write, then `send_datagram`s sized to the
remaining headroom) and asserts it, because a construction that leaves
1 086 B of headroom emits the coalesced ACK+`PATH_CHALLENGE` packet, arms
the death clock, and the connection dies — my first run did exactly that,
and it is the most likely way to build this and conclude "not reachable".


## 4. Core-level result — **THE CONSTRUCTION SUCCEEDS**

```
$ cargo test --lib probe_the_construction -- --nocapture
running 1 test
[installed]           budget=None       room=None    armed=Some(true)  passive_debt=Some(false) in_flight=0     cwnd=12000 timers: Liveness=true  Keepalive=false Beacon=false Pto=false Loss=false AckDelay=false Contested=false
[after first delivery] budget=None      room=None    armed=Some(false) passive_debt=Some(true)  in_flight=0     cwnd=12000 timers: Liveness=false Keepalive=true  Beacon=false Pto=false Loss=false AckDelay=false Contested=false
[window fragmented]   budget=None       room=None    armed=Some(false) passive_debt=Some(true)  in_flight=0     cwnd=12000 timers: Liveness=false Keepalive=true  Beacon=false Pto=false Loss=false AckDelay=false Contested=false
[wrote] blocked 0 times, 8 transmits, deadline_some=true
[in flight]           budget=None       room=None    armed=Some(true)  passive_debt=Some(false) in_flight=9328  cwnd=12000 timers: Liveness=true  Keepalive=false Beacon=false Pto=true  Loss=false AckDelay=false Contested=false
[topup] want=1000 transmits=[1000] in_flight 9328 -> 10328
[topup] want=1000 transmits=[1000] in_flight 10328 -> 11328
[topup] want=672  transmits=[672]  in_flight 11328 -> 12000
[topup] final headroom = 0
[gate closed]         budget=None       room=None    armed=Some(true)  passive_debt=Some(false) in_flight=12000 cwnd=12000 timers: Liveness=true  Keepalive=false Beacon=false Pto=true  Loss=false AckDelay=false Contested=false
[roam drain] transmits=0 sizes=[] deadline_some=true
[roam event] AddressMoved { from: 10.0.0.1:1, to: 10.0.0.9:41000 }
[roam event] SendCreditAvailable
[post-roam]           budget=Some((0, 35))   room=Some(105) armed=Some(false) passive_debt=Some(true) in_flight=12000 cwnd=12000 timers: Liveness=false Keepalive=true Beacon=false Pto=true Loss=false AckDelay=true Contested=false
[driver] step 0 @25ms sent 105 B, frames [Ack { largest: 96, ack_delay: 25000, ranges: [(0, 0) x 33], first_range: 1 }]
[driver step 0 @25ms] budget=Some((105, 35)) room=Some(0)   armed=Some(false) passive_debt=Some(true) in_flight=12000 cwnd=12000 timers: Liveness=false Keepalive=false Beacon=false Pto=false Loss=false AckDelay=false Contested=false
[driver] parked at step 1, 25ms after start
[driver] sent 1 packets after the roam
[+60s] transmits=0 closed=None deadline_some=false
[+60s]                budget=Some((105, 35)) room=Some(0)   armed=Some(false) passive_debt=Some(true) in_flight=12000 cwnd=12000 timers: Liveness=false Keepalive=false Beacon=false Pto=false Loss=false AckDelay=false Contested=false
DEAD_TIMEOUT = 25s
test core::connection::tests_park::probe_the_construction ... ok
```

**Every one of C's six steps reproduces, in order, at the core.** The
terminal state, measured 25 ms after start and re-measured at +60 s
(2.4x `DEAD_TIMEOUT`):

* `Timeout(None)` — **no timer armed at all** (`Liveness`, `Keepalive`,
  `PersistentKeepalive`, `Pto`, `Loss`, `AckDelay`, `Contested` all `None`);
* `liveness.is_armed() == false` — the death clock is **not armed**, so
  `Liveness::deadline()` is `None`, not "suppressed";
* `liveness.owes_passive_keepalive() == true` — the keepalive **is owed**;
* `room == 0` — `keepalive_can_leave()` is false, so the one send that
  could re-arm the death clock is vetoed;
* at +60 s: **0 transmits, `closed() == None`.** The connection did not die
  and did not send.

The one packet that leaves after the roam is the pure ACK, and it is
exactly the mechanism C predicted and the blind author's `the_passive_form`
denied: **105 bytes — the entire 3x35 room — carrying 33 ACK range pairs**,
sealed `seal_quiet`, neither marking nor ack-eliciting. Not "~35 bytes".
`ack::derive` filled it to `Packing::room()`.

**C's §4.2.1 is confirmed by measurement. The predicted failing link
(step 4/5, the ACK's reachable size) did not fail.**


## 5. Shell / FlakyWire end-to-end — **CONFIRMED, first attempt**

`tests/round41_park.rs` (temporary), two real endpoints over `FlakyWire`,
`#[tokio::test(start_paused = true)]`. The construction transposes cleanly:

| core lever | shell lever |
|---|---|
| burn a counter, deliver | `FlakyPolicy::lossy(0.5)` on B's wire for 160 small B writes -> ~half the counters never arrive |
| freeze the window | `FlakyPolicy::lossy(1.0)` — datagrams are **tapped** (step 4) and *then* dropped (step 5), so a blocked path would not work but 100 % loss does |
| flight >= cwnd | A writes 200 KB with every ACK from B lost; A's own PTO probes are **cwnd-exempt** and keep adding to `bytes_in_flight`, so the gate shuts on its own — no byte-exact top-up needed |
| `deliver_from(c_addr)` | `Network::inject(addr_c, a_addr, fresh)` with a **fresh, never-delivered** tapped b->a datagram (a replay would not roam, §7.2) |

```
$ cargo test --features test-util --test round41_park -- --nocapture
running 1 test
[phase1] a_addr=10.0.0.1:4001 b_addr=10.0.0.2:4002
[phase2] a wrote 204800 bytes into the stream
[phase2] tap saw 64553 B of a->b datagrams in total
[phase3] injecting a 39 B b->a datagram from 10.0.0.3:4003
[phase3] a sent [(10.0.0.3:4003, 117)] after the roam
[measure] after 60s of virtual time: still_alive=true, a sent []

thread 'a_starved_passive_keepalive_parks_the_shell_forever' panicked at tests/round41_park.rs:155:9:
§7.4: a connection whose peer has vanished must be reaped at DEAD_TIMEOUT (25s).
It was still alive after 60s and sent 0 packet(s) in that time.
```

**The arithmetic is visible in the trace**: the injected roam packet is
39 B, so `room = 3 x 39 = 117`; A emits **exactly one 117-byte packet** —
the whole room, in one pure ACK — and then nothing, for 60 s of virtual
time, 2.4x `DEAD_TIMEOUT`, without dying. `remote_address()` is `addr_c`,
so the roam premise is pinned too.


## 6. Fix candidates

The three the brief names, judged against the ratified text I actually
opened (working rule 11: a claim about the spec is unchecked until the
section is read).

**The two normative sentences the whole question turns on.**

`SPEC.md:2586-2596` (§7.4):

> The connection is dead when `now - last_authenticated_recv >=
> DEAD_TIMEOUT` **and** at least one **arming** send has occurred since
> that last authenticated receive.

`SPEC.md:2690-2696` (§7.5, the ruling-182 amendment):

> the beacon's soundness proof below rests on *"every send that can
> establish `S > R` is a marking send, so the death clock is armed there
> (§7.4)"*. Under the prose reading a **non-marking** send blocks the
> dance ... **without arming the death clock**, and the proof collapses
> into exactly the immortal half-open session SECV5-2 was applied to
> prevent.

**§7.4's second conjunct is only sound because §7.5 promises an arming
send within `KEEPALIVE_TIMEOUT` of every receive.** The measured defect is
that the shipped announce-gate introduces a **second** way to block the
dance — §7.3's budget — and that one does *not* arm the death clock. It is
ruling 182's collapse reached through a door ruling 182 did not enumerate:
working rule 8's shape exactly (*a stated construction with an unstated
scope* — "every send that can establish `S > R`" quantifies over sends,
and says nothing about a keepalive that is **owed and never sent**).

An equivalence worth having, because it makes the fix's scope provable:

* `armed` is set by any marking **or** ack-eliciting send; the passive
  debt is cleared **only** by a marking send (`session.rs:320-333`).
* So after a marking send: `armed && !debt`. After an ack-eliciting
  non-marking send: `armed && debt`. After a receive: `!armed && debt`.
* Therefore **`!armed` implies `debt`**, everywhere except the install pin
  (`armed && !debt`, `session.rs:290`).

That is what makes the backstop below total rather than a patch on one
path: there is no state with the death clock disarmed and no keepalive
owed.

### 6a. Arm `Liveness` whenever passive debt is set

Two readings, and they are very different.

* **(a1) never disarm** — drop `self.armed = false` from
  `on_authenticated_fresh_recv`. This deletes §7.4's second conjunct
  outright, which is *ratified normative text amended as recently as
  ruling 85*. It also changes nothing observable on a healthy connection
  (the dance arms within `KEEPALIVE_TIMEOUT` = 10 s, well inside
  `DEAD_TIMEOUT` = 25 s), so its entire effect **is** the starved case —
  which is an argument that the conjunct is doing no work, not an argument
  that deleting it is a small change. Needs a ruling on §7.4 itself.
* **(a2) arm only where the gate suppresses** — indistinguishable in code
  from 6c below; see there.

### 6b. Exempt the passive keepalive from the gate

**Reject, and the reason is §7.3, not §7.5.** Two sub-forms, both bad:

* *Exempt the transmit guard too* (so the keepalive actually leaves):
  §7.3 says the budget *"binds all output and cannot be waived"*, and
  `SPEC.md:2419-2421` repeats it against §14.5's and §13.4's cwnd
  exemptions specifically. A 30-byte keepalive that ignores the budget
  turns the session into an unbounded reflector aimed at whatever address
  last roamed it — §7.3's entire threat model.
* *Exempt only the announcement, keep the guard*: that is F1's spin
  verbatim — a deadline re-armed at `last_send + KEEPALIVE_TIMEOUT`, an
  instant already past, `handle_timeout` re-firing forever. Ruling 220
  chose suppression over it deliberately, and `tests_livelock.rs` §2 pins
  it. Reintroducing it trades an immortal connection for a spinning one,
  which ruling 220 already called the worse trade in the other direction.

### 6c. `Liveness`-at-the-latest backstop (ruling 249's shape) — **the candidate**

**The timer is `last_authenticated_recv + DEAD_TIMEOUT`** — i.e. exactly
what `Liveness::deadline()` would return if `armed` were true. In one
sentence: *a keepalive that is owed but that §7.3 will not let leave arms
the death clock in its own right.*

Why this is the right shape rather than a patch:

1. **It is sound in the ruling-249 sense, and for the same reason.** Both
   disjuncts of `keepalive_can_leave()` lift **only on a receive** — the
   budget grows only on an authenticated, window-fresh receive
   (ruling 169), and a contested mark clears only on an ACK covering its
   floor. A receive re-derives `last_authenticated_recv`, so the backstop
   deadline moves with it and never fires on a connection that is still
   hearing from its peer. If nothing arrives for `DEAD_TIMEOUT`, the
   connection dies — which is precisely §7.4's intent.
2. **It suppresses nothing and adds no timer kind.** Ruling 249's clause
   withheld an announcement; this one supplies one. Same seam
   (`sync_liveness_timer`), same "a deadline is never announced for output
   that cannot leave" principle read in the other direction.
3. **It leaves §7.4's normative sentence intact.** The connection still
   dies only when an arming *event* has occurred since the last receive —
   the change is that an **owed-and-vetoed keepalive counts as one**. That
   is a one-clause amendment to §7.4/§7.5, not a deletion.


### 6d. Applied candidate + measurement

Applied in this worktree at `1105df9` (`src/core/connection/mod.rs`,
`sync_liveness_timer`). **The whole change is 5 lines of code plus its
comment:**

```rust
let clocks = self.session.as_ref().map(Session::liveness).copied();
let can_leave = self.keepalive_can_leave();

let backstop = clocks.filter(|l| !can_leave && l.owes_passive_keepalive());
let deadline = clocks
    .and_then(|liveness| liveness.deadline())
    .or_else(|| backstop.map(|l| l.last_authenticated_recv() + constants::DEAD_TIMEOUT));
self.timers.set(TimerKind::Liveness, deadline);
```

**Which behaviour does the fix produce — death or a probe? Death, at
exactly `DEAD_TIMEOUT`.** Measured, same construction, same driver loop:

```
$ cargo test --lib tests_park -- --nocapture
[post-roam] budget=Some((0, 35)) room=Some(105) armed=Some(false) passive_debt=Some(true) ... Liveness=false ...
[driver] step 0 @25ms sent 105 B, frames [Ack { largest: 96, ... 33 range pairs }]
[driver] parked at 25s, sent 1, died=true          <-- was `parked at 25ms ... died=false`
test core::connection::tests_park::a_starved_passive_keepalive_must_not_park_the_connection_forever ... ok
test core::connection::tests_park::probe_the_construction ... ok
test core::connection::tests_park::the_backstop_does_not_reap_a_connection_whose_peer_comes_back ... ok
test result: ok. 3 passed; 0 failed; 744 filtered out
```

and at the shell:

```
$ cargo test --features test-util --test round41_park -- --nocapture
[phase3] injecting a 39 B b->a datagram from 10.0.0.3:4003
[phase3] a sent [(10.0.0.3:4003, 117)] after the roam
[measure] after 60s of virtual time: still_alive=false, a sent []
test a_starved_passive_keepalive_parks_the_shell_forever ... ok
```

**The other half — the fix must not reap a connection whose peer is only
briefly starving it** (working rule 12: check what state the argument
assumed). `the_backstop_does_not_reap_a_connection_whose_peer_comes_back`
parks the connection, then lets the peer speak again at +10 s:

```
[recovery] parked at 25ms; Liveness announced at Some(25s) after start
[recovery] after the peer returns: room=Some(105) armed=false debt=true Keepalive=Some(10s) Liveness=None
[recovery] after the return: sent=2 died=true at 35s
```

The receive re-funds the budget (`room 0 -> 105`), the backstop deadline
disappears because the *real* keepalive announcement returns in its place,
two keepalives leave, and the connection dies at **35 s = 10 s + 25 s** —
`DEAD_TIMEOUT` from the last authenticated receive, exactly §7.4.

**One fixture note, not a defect.** At the instant the peer returns, the
`Keepalive` deadline announced is `last_send + KEEPALIVE_TIMEOUT`, which
is already reached (`Keepalive=Some(10s)` at `now = 10s`). That is a
single catch-up turn — the timer fires, the keepalive seals, `last_send`
moves — not F1's spin, which is *the same value re-announced forever*. The
driver harness bounds consecutive non-advancing turns rather than
forbidding one, and the shipped `tests_livelock.rs` assertion
(`next > now`) would flag this instant if it were ever pointed at it.


## 7. Suite impact — **the fix moves exactly the new tests, nothing else**

Measured both ways on the same tree, my three core tests and one shell
test present in both runs.

**Fix reverted** (`git checkout 878104f -- src/core/connection/mod.rs`):

```
$ cargo test --all-features --no-fail-fast
test core::connection::tests_park::a_starved_passive_keepalive_must_not_park_the_connection_forever ... FAILED
test result: FAILED. 745 passed; 1 failed; ...
test a_starved_passive_keepalive_parks_the_shell_forever ... FAILED
test result: FAILED. 0 passed; 1 failed; ...
   ... every other suite: ok, identical counts
```

**Fix applied:**

```
$ cargo fmt --all --check
FMT OK
$ cargo clippy --all-features --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.14s     (zero warnings)
$ cargo test --all-features
test result: ok. 747 passed; 0 failed; ...        (lib; 744 pre-existing + my 3)
test result: ok. 1 passed; 0 failed; ...          (round41_park)
   ... 24 / 112 / 11 / 4 / 5 / 12 / 20 / 4 / 10 / 12 / 7 / 12 / 16 / 15 / 16 / 4 / 17 / 6 / 6 / 12 — all ok
$ cargo test --release --all-features
test result: ok. 747 passed; 0 failed; ...        (and every suite ok, incl. the debug-ignored S23)
```

**Nothing in the 744 pre-existing lib tests or the ~330 integration tests
moves.** In particular `tests_livelock.rs`'s eight F1 tests, `tests_roam.rs`,
`tests_pto_gate.rs` (ruling 249's own gate), `story_keepalive.rs` and
`story_mobility.rs` are all unaffected — which is the expected result,
since the backstop only supplies a deadline where the shipped build
supplied `None`.


## 8. The separating assertion a permanent regression should pin (rule 9)

**Three assertions, deliberately separate**, in
`src/core/connection/tests_park.rs::a_starved_passive_keepalive_must_not_park_the_connection_forever`:

1. **Premise** — read at the instant the starving pure ACK lands, not at
   the end (on a build that then kills the connection, `liveness()` is
   already `None` there — my first draft made exactly that mistake and
   panicked on `expect`):
   `owes_passive_keepalive() && !is_armed() && room < 30`.
   *Without it the test passes vacuously on any construction that quietly
   failed to reach the state* — the failure mode working rule 9 names.
2. **Liveness** — within `DEAD_TIMEOUT + slack`, driving only the
   announced deadlines, the connection has **either died or sent
   something**. It deliberately does **not** demand death: a fix that
   re-arms the keepalive, and a fix that supplies a `Liveness` backstop,
   both pass.
3. **Non-retrospection** — every announced deadline is at or after the
   `now` just handled, and no more than a bounded number of consecutive
   turns fail to advance. Inherited from `tests_livelock.rs`: the fix must
   not buy termination with F1's spin.

**The builds this separates, which is the whole point:**

| build | fails on |
|---|---|
| **shipped** (`94dab20`) — announces `Timeout(None)`, neither dies nor sends | **liveness** |
| "fix" by removing the gate from the keepalive *announcement* only (keep the transmit guard) | **non-retrospection** — F1's spin, re-armed in the past forever |
| a build that suppresses `Liveness` too, everywhere | **liveness** here, and **premise** nowhere |
| a build whose construction silently fails (e.g. §14.5's gate left open, so the coalesced ACK+`PATH_CHALLENGE` leaves and arms the clock) | **premise** — caught by `assert_eq!(cwnd - in_flight, 0)` in the fixture and by `!is_armed()` |
| the candidate fix (6c) | **nothing** — dies at exactly `DEAD_TIMEOUT` |

A fourth, added because the fix needs its own separator: **the backstop
must not reap a connection whose peer comes back**
(`the_backstop_does_not_reap_a_connection_whose_peer_comes_back`). A
degenerate "fix" that simply announced `Liveness` unconditionally passes
assertions 1–3 and fails this one.

**Where it should live permanently.** The core test belongs in
`tests_livelock.rs` §5 — it is the very hole that file's `the_passive_form`
reports as *"not constructible"*, and the doc comment there needs
correcting whether or not the fix lands (its second step, *"a pure ACK …
costs ~35 bytes"*, is false: the ACK is **sized to the room**, measured at
105 B and 117 B). The shell test belongs beside `story_keepalive.rs`'s S5.


## 9. Verdict: **UNSOUND — parkable forever**

**The shipped announce-gate is UNSOUND.** A connection can reach a state
in which it owes §7.5's passive keepalive, §7.3's budget vetoes it, §7.4's
death clock is disarmed, **no timer is armed at all**, and the connection
therefore neither dies nor sends — for ever.

**The measured evidence line, core:**

```
[driver] parked at 25ms, sent 1, died=false
[parked] budget=Some((105, 35)) room=Some(0) armed=Some(false) passive_debt=Some(true) \
         timers: Liveness=false Keepalive=false Beacon=false Pto=false Loss=false AckDelay=false Contested=false
[+60s]   transmits=0 closed=None deadline_some=false
```

**and shell:**

```
[phase3] a sent [(10.0.0.3:4003, 117)] after the roam
[measure] after 60s of virtual time: still_alive=true, a sent []
```

60 s is 2.4x `DEAD_TIMEOUT`. This is the **immortal half-open session**
that ruling 182's beacon proof exists to forbid, and the same shape
ruling 195 fixed once already — reached, as the brief anticipated, through
a different door.

**Three things the ruling needs, beyond the verdict:**

1. **The announce-gate did not create this, and removing it does not fix
   it** — C-keepalive-gate.md §4.2.2 is right, and I did not re-measure
   that half. Pre-F1 the same state *spins* instead of parking; neither
   dies. The gate is a strict improvement and the defect is upstream of
   it: §7.4's `armed` bit and §7.5's passive debt are set by the **same
   event**, so the passive keepalive is the only thing that can arm the
   death clock in its own window, and §7.3 can veto it.
2. **Ruling 249's soundness sentence must not be copied into a keepalive
   clause.** *"the connection's `Timeout` falls to the next armed timer —
   `Liveness` at the latest"* is true for `Pto` (a `Pto` is armed only
   when the sent map is non-empty, which means an ack-eliciting send
   happened, which means `armed`) and **measurably false** for the passive
   keepalive. Drafting it in would ship a false rationale.
3. **`tests_livelock.rs:873-928` (`the_passive_form`) must be corrected
   whatever is decided.** Its obstruction's *first* step is right and I
   verified it; its **second** step — *"A pure ACK … costs ~35 bytes
   against the >= 90 its own trigger credited: the room grows
   monotonically"* — is **false**, and is the whole load-bearing claim.
   `packing()` clamps the plaintext to `room - 30` and `ack::derive`
   truncates newest-first *at that room*: measured at **105 B against a
   105 B room** (core, 33 range pairs) and **117 B against a 117 B room**
   (shell). The room does not grow monotonically; one non-marking,
   non-arming packet takes all of it.

---

## 10. What is in the worktree, and what is not

Commits on `worktree-agent-adb13e14fdf82c5cd`, all prefixed `TEMP`,
**none of them a proposal to merge** — evidence for a ruling:

| commit | what |
|---|---|
| `6458660` | the core measurement harness (`src/core/connection/tests_park.rs` + its `mod` line) |
| `878104f` | the shell E2E (`tests/round41_park.rs` + its `Cargo.toml` stanza) |
| `1105df9` | **candidate fix (c)** — 5 lines in `sync_liveness_timer` |
| `a67e518` | the recovery test, `cargo fmt` |

Base `94dab200a172a5b62b4d28711962574d61a9d3c8`, verified before the first
read. Gates run on the final tree: `cargo fmt --all --check` clean,
`cargo clippy --all-features --all-targets -- -D warnings` zero warnings,
`cargo test --all-features` and `cargo test --release --all-features` all
green.

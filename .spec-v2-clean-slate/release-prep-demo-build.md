# Demo build: slither in the browser (ruling 277 MVP)

Working notes, appended as the work proceeded (project rule 2 — the
skeleton below existed before the first `Read`). The stub headings it
started with are now the filled sections further down; this header keeps
the base record and an index.

## Base

- Worktree: `.claude/worktrees/agent-aa0c4d11c65142fd1`
- Branch: `worktree-agent-aa0c4d11c65142fd1`
- `git log -1 --oneline` → `ca89a83 Ruling 277: wasm is a compile target, not a wire`
- `git status --porcelain` → empty (clean)
- Expected base ca89a83: **CONFIRMED** (rule 14's first act, before any
  other command).

## Build log — what was built, in order

1. Inventory of `testutil`, the `Wire` seam and the §18.2 trace targets,
   and confirmation that ruling 277's two hunks are actually committed at
   `ca89a83` rather than assumed from the feasibility report.
2. `demo/runner/` — a `wasm32-wasip1` command binary, its own unpublished
   crate: `log.rs` (the event stream), `wire.rs` (`ObservedWire`, the fault
   injector that can report what it did), `trace.rs` (a `tracing`
   subscriber stamping virtual time), `sim.rs` (two endpoints and §6.2's
   ladder, climbed audibly), `scenarios.rs`, `main.rs`.
3. Native runs of all four scenarios; two defects found and fixed.
4. wasm build (628 KB), runs under `node:wasi`, identical timelines.
5. `@bjorn3/browser_wasi_shim` 0.4.2 vendored with licences and hashes.
6. `demo/web/` — framework-free page: `wasm-run.js` (the one starter,
   shared by the Worker and the verifiers), `worker.js`, `app.js`
   (an animated sequence diagram on canvas), `index.html`, `style.css`.
7. `demo/verify/` — four harnesses, all green.
8. `.github/workflows/pages.yml`, `demo/build.sh`, `Cargo.toml`'s one
   `exclude` entry, `demo/README.md`.

## Index

| section | what is in it |
|---|---|
| Inventory | the API measured before any code was written |
| Architecture | the fate-reconstruction problem and `ObservedWire` |
| Event schema | the JSON-line contract between the runner and the page |
| Verification runs — native | all four scenarios, and two defects found by running them |
| Verification runs — wasm | `node:wasi`, the vendored shim, the page's own logic |
| Main-crate gates | proof the library is untouched |
| Findings for the integrator | three things reported and not acted on |
| Deviations from the brief | four, with reasons |
| Remaining work | what only a browser can settle, and what to build next |

---
## Inventory (measured, before writing any code)

Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `node v22.22.2`.
`rustup target list --installed` includes **`wasm32-wasip1`** (and
`wasm32-unknown-unknown`, unused here).

### Ruling 277's two hunks are present at ca89a83 — verified, not assumed

- `Cargo.toml:132` → `tokio = { version = "1", features = ["rt", "time",
  "sync", "macros"] }` (no `net`), and `Cargo.toml:189` →
  `[target.'cfg(not(target_family = "wasm"))'.dependencies]` re-adding
  `tokio` with `features = ["net"]`.
- `src/shell/wire.rs:80` → `#[cfg(not(target_family = "wasm"))]` above the
  `impl Wire for tokio::net::UdpSocket`.

So the wasm build the feasibility report proved on a throwaway probe is
**the committed state**. I change no `src/` file.

### The API the runner builds against — all already public

`slither::testutil` (feature `test-util`), from `src/testutil/mod.rs`:

| item | what the demo uses it for |
|---|---|
| `Network::seeded(u64)` | the fabric; one seed drives loss/delay draws **and** both endpoints' RNGs and static keys |
| `Network::tap() -> Tap` | the packet animation's source |
| `Tap::drain() -> Vec<Spied>` / `datagrams()` | `Spied { src, dst, bytes }` per accepted send |
| `Network::sends()` | total accepted sends counter |
| `Network::partition/heal/block_path/heal_path` | scenario 4 |
| `FlakyPolicy::{perfect, lossy, drop_first, drop_at, with_delay, with_duplication}` | the sliders |
| `Pair::seeded(u64)` → `{ net, a, b }`, `Peer { endpoint, wire, dhs, public_static }` | the two endpoints |
| `Peer::addr()`, `Peer::rebind()` | roaming (not in MVP) |
| `DhCounter::get()` | the 0→1→2→4 DH ladder counter |
| `Capture::install()`, `CapturedEvent { target, level, message, fields }` | §18.2 trace → demo events |
| `addr_a()` = `10.0.0.1:4001`, `addr_b()` = `10.0.0.2:4002` | column labels |

`Tap` semantics, quoted from its own doc — **this bounds what the
animation can honestly claim**:

> "Accepted" means the send was not refused by an injected send failure
> and was not blackholed by a partition — it is what left the wire, which
> is a different question from what arrived. **Loss and duplication are
> applied *after* the tap.**

So the tap sees *departures*, not arrivals. A dropped datagram appears in
the tap exactly like a delivered one. Consequence for the demo: **the tap
alone cannot tell me which packets died.** See "Fate reconstruction"
below — this is the one real design problem in the whole build.

### §18.2 trace targets, counted from source

`grep -rho 'target: "slither::[a-z]*"' src/ | sort | uniq -c`:
`slither::policy` 11 · `slither::io` 8 · `slither::frames` 5 ·
`slither::roam` 3 · `slither::replay` 1.

The single `slither::replay` site is `src/core/connection/session.rs:612`,
`tracing::debug!(target: "slither::replay", counter, greatest = ?…,
"a received packet was rejected by the replay window")` — which is
**exactly** scenario 3's payoff event, available as data with no new code.

### `block_on` will NOT be used

`src/compat/rt.rs:75` builds `new_current_thread().enable_all()` — no
`start_paused`. The demo needs the paused clock (it is both the
fast-forward *and* what keeps the browser shim off `poll_oneoff`), so the
runner builds its own runtime exactly as the feasibility report's
`wasidemo2` did. Deviation from the brief's wording ("real shell:
`slither::block_on`"), and a deliberate one: the *shell* is real either
way; only the four-line runtime builder differs, and the brief's own
architecture section mandates `start_paused`. The two requirements are
not simultaneously satisfiable with `block_on` as written.

---

## Architecture

### The one real design problem: **fate reconstruction**

The brief asks for "packets as dots travelling between them (dropped ones
visibly dying mid-flight)". `Tap` cannot supply that. Re-reading
`FlakyWire::send_to`'s own documented step order (`src/testutil/mod.rs`,
the `impl Wire for FlakyWire` doc block):

> 4. Record in the tap.
> 5. Decide how many copies to deliver: 0 (lost), 1, or 2 (duplicated).
> 6. Draw a delay per copy and queue it.

The tap fires at step 4 and the fate is decided at step 5, so a tapped
`Spied` is indistinguishable between delivered, lost and duplicated. Nor
is the delay observable, so there is no arrival time either. **A demo
built on the tap alone would have to *invent* which dots die** — which is
exactly the kind of thing this page must not do.

Three options were considered:

1. *Mirror `FlakyWire`'s draws in the demo.* Rejected. The draw order is
   documented as determinism contract, but the RNG seeding
   (`seed ^ ordinal.wrapping_mul(SEED_STRIDE)`, `ChaCha20Rng`) is private
   and re-deriving it would couple the demo to internals and silently
   desynchronise the moment anything changed.
2. *Show departures only, and never claim a fate.* Honest, but it deletes
   scenarios 2 and 3's whole payoff.
3. **Wrap the wire.** Chosen.

### `ObservedWire` — the chosen mechanism

`Wire` is a public trait (`slither::shell::Wire`, `src/shell/wire.rs:70`)
whose normative property 1 is *"the application supplies it … or a
**simulator** installs one without forking the crate"*. The demo is
precisely that consumer. So:

- `demo/runner/src/wire.rs` defines `ObservedWire`, which holds an
  `Rc<FlakyWire>` **left on `FlakyPolicy::perfect()`** and applies loss,
  duplication and delay itself, from its own tiny SplitMix64 stream seeded
  off the scenario seed.
- Because the demo makes the decision, the demo *knows* it — departure
  time, arrival time, and fate, per copy, exactly.
- The `Network` still does everything else: registration, routing,
  inboxes, `recv_from`'s `sleep_until` (the line that makes virtual time
  work), partition/blackhole, `notify`.
- Delay is realised by `tokio::task::spawn_local`-ing a task that
  `sleep_until`s the drawn arrival instant and only then calls the inner
  wire — never by awaiting inside `send_to`, which would stall the driver
  and distort protocol timing.

Nothing about slither's behaviour changes: the driver sees a `Wire` that
sometimes loses datagrams, which is the whole of what `FlakyWire` was
doing to it before.

**Consequence to state plainly:** `Network::sends()` and `Tap` now count
the demo's *survivors* rather than its attempts, so the runner publishes
its own counters and does not use the tap at all. The tap is not wrong; it
is answering a different question.

### Timestamps and the event log

One shared `Arc<Mutex<Vec<String>>>` log, written by three producers, so
global emission order is preserved:

1. the scenario body (stage transitions, application I/O, counters),
2. `ObservedWire` (one event per datagram copy),
3. a **`tracing::Subscriber` written in the runner** that stamps
   `tokio::time::Instant::now()` at event time.

`testutil::Capture` was considered for (3) and **not** used: a
`CapturedEvent` carries `target`/`level`/`message`/`fields` and **no
timestamp**, and draining it at await boundaries would place §18.2 events
at the drain instant rather than the emission instant, and out of order
with respect to (1) and (2). The runner's subscriber is ~60 lines of the
same shape as `src/testutil/capture.rs` and gets both right. (Recorded so
nobody later "simplifies" it back to `Capture` without knowing the cost.)

All `t_us` values are **virtual** microseconds since the scenario's first
instant, read from `tokio::time::Instant::now()` under
`start_paused(true)`.

### Runtime

```rust
tokio::runtime::Builder::new_current_thread()
    .enable_time()
    .start_paused(true)
    .build()
```
driven by `LocalSet::block_on`. Not `slither::block_on` — see the
inventory note above.

### Why the paused clock is load-bearing for the *browser*, measured

The vendored shim's `poll_oneoff` is a **busy-wait spin**:

```js
poll_oneoff(in_ptr,out_ptr,nsubscriptions){ …
  const endTime = … ; while(endTime>getNow()){} … }
```

(`demo/web/vendor/browser_wasi_shim/wasi.js`, read from the published
0.4.2 dist.) If the runtime ever parked on a real timer, the Worker would
spin-burn a core for the whole wall duration — a 10 s retransmit ladder
would be a 10 s frozen spin. Under `start_paused(true)` tokio auto-advances
and never parks, so `poll_oneoff` is never called. The feasibility report
inferred this mattered; the shim's source says exactly *why*, and the node
harness below **asserts the call count is zero**.

---

## Event schema

One JSON object per line on stdout. Every object carries `seq` (a
monotonic counter across all producers) and `t_us` (**virtual**
microseconds since the run's first instant).

| `kind` | fields | meaning |
|---|---|---|
| `run` | `scenario`, `seed`, `loss`, `duplicate`, `delay_us`, `jitter_us`, `message` | the parameters, echoed back |
| `topology` | `a`, `b` | the two endpoint addresses |
| `stage` | `side`, `stage`, `dhs:{a,b}`, plus per-stage extras | §6.2's ladder: `dial` · `intro` (+`source`, `sender_index`) · `claimed` · `proven` (+`timestamp`) · `connection` · `established` |
| `tx` | `pkt`, `copy`, `from`, `to`, `dst`, `len`, `type`, `fate`, `arrive_us`? | one datagram **copy**. `type` ∈ `init`/`resp`/`data`/`malformed`/`empty`; `fate` ∈ `delivered`/`lost`/`blackholed`; `arrive_us` only when delivered |
| `trace` | `target`, `level`, `message`, `fields` | a §18.2 event, verbatim |
| `app` | `side`, `op`, `bytes`?, `text`?, `code`?, `reason`? | application I/O: `sent`/`received`/`close` |
| `lost` | `side`, `cause`, `silent_us`? | a `ConnectionLost`, rendered by its own `Display` |
| `fault` | `what`, `detail` | the demo changing the network |
| `note` | `text` | one line of narration |
| `end` | `ok`, `error`?, `wall_us`, `sent`, `delivered`, `lost`, `duplicated`, `blackholed` | the summary; **always last** |
| `error` | `message` | argv rejected before the runtime was built |

`pkt` is numbered from the **log**, not per wire, so it is unique across
both endpoints; `copy` is 0 or 1 and a `copy:1` is a duplicate of the
`copy:0` with the same `pkt`.

## Verification runs — native

All four scenarios, `cargo run` on aarch64-apple-darwin. Full commands and
output.

### `cargo test` (runner's own units)

```
$ cargo test
running 8 tests
test tests::defaults_hold_and_flags_override ... ok
test tests::a_missing_value_or_unknown_flag_is_an_error ... ok
test tests::an_out_of_range_probability_is_rejected_rather_than_clamped ... ok
test tests::the_message_is_length_capped ... ok
test log::tests::escapes_the_mandatory_two_and_the_control_range ... ok
test wire::tests::a_handshake_byte_with_the_wrong_length_is_not_labelled_a_handshake ... ok
test wire::tests::jitter_actually_varies_and_zero_jitter_does_not ... ok
test wire::tests::the_unit_draw_spans_the_range_and_stays_inside_it ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Scenario 2 — the lossy handshake, which is the whole point

```
$ ./target/debug/slither-demo --scenario lossy --loss 0.35 --seed 6 \
    | grep -E '"type":"init"|"type":"resp"|"stage"|"kind":"end"'
{"seq":2,"t_us":0,"kind":"stage","side":"a","stage":"dial","dhs":{"a":0,"b":0}}
{"seq":3,"t_us":0,"kind":"tx","pkt":0,…,"len":196,"type":"init","fate":"lost"}
{"seq":4,"t_us":5034000,"kind":"tx","pkt":1,…,"len":196,"type":"init","fate":"lost"}
{"seq":5,"t_us":10113000,"kind":"tx","pkt":2,…,"len":196,"type":"init","fate":"delivered","arrive_us":10113000}
{"seq":6,"t_us":10113000,"kind":"stage","side":"b","stage":"intro","dhs":{"a":6,"b":0},…}
{"seq":7,"t_us":10113000,"kind":"stage","side":"b","stage":"claimed","dhs":{"a":6,"b":1}}
{"seq":8,"t_us":10113000,"kind":"stage","side":"b","stage":"proven","dhs":{"a":6,"b":2},…}
{"seq":9,"t_us":10113000,"kind":"tx","pkt":3,…,"len":107,"type":"resp","fate":"delivered","arrive_us":10113000}
{"seq":10,"t_us":10113000,"kind":"stage","side":"b","stage":"connection","dhs":{"a":8,"b":4}}
{"seq":11,"t_us":10113000,"kind":"stage","side":"a","stage":"established","dhs":{"a":8,"b":4}}
{"seq":39,"t_us":14359000,"kind":"end","ok":true,"wall_us":54881,"sent":22,"delivered":13,"lost":9,…}
```

Read what that says. Two `INIT_PACKET_LEN`-196 datagrams die; the
retransmits land at **5.034 s** and **10.113 s** of virtual time —
§5.5's `REKEY_TIMEOUT` ladder with its jitter, on the ratified timer,
nobody's stopwatch involved. And `a`'s DH count is **6** at the moment
`intro` fires rather than 2: each retransmit carries a **fresh
ephemeral**, so the ladder is priced three times. That is a better
annotation than anything I would have written by hand, and it is a
measurement.

The whole 14.36-second run cost **54.9 ms of wall time**.

*Correction to the brief:* it predicts the DH ladder is "0→1→2". Measured,
the initiator goes 0→2 on msg1 and 2→4 on msg2, and the responder
0→1→2→4 across the four rungs — §6.1 prices the ladder cumulatively and
the initiator pays two at a time. The demo reports both counters at every
rung rather than asserting a shape.

### Scenario 3 — duplication, and §7.2 refusing it

```
$ ./target/debug/slither-demo --scenario duplicate --duplicate 0.6 --seed 3 \
    | grep -E 'replay|"copy":1' | head -6
{"seq":4,…,"pkt":0,"copy":1,…,"type":"init","fate":"delivered","arrive_us":0}
{"seq":9,…,"pkt":1,"copy":1,…,"type":"resp","fate":"delivered","arrive_us":0}
{"seq":15,…,"pkt":2,"copy":1,…,"type":"data","fate":"delivered","arrive_us":0}
{"seq":18,"t_us":0,"kind":"trace","target":"slither::replay","level":"DEBUG",
 "message":"a received packet was rejected by the replay window",
 "fields":{"counter":"0","greatest":"Some(0)"}}
{"seq":21,…,"pkt":5,"copy":1,…,"type":"data","fate":"delivered","arrive_us":0}
{"seq":23,"t_us":0,"kind":"trace","target":"slither::replay",…,
 "fields":{"counter":"2","greatest":"Some(2)"}}
```

The duplicate arrives, and slither says so on its own §18.2 target with
the counter that was refused. Worth noticing: the **handshake**
duplicates (`pkt` 0 and 1) produce no replay trace — they are refused by
different machinery — which is a true and non-obvious thing the page can
show without anyone inventing it.

### Scenario 4 — partition, and death at exactly the ratified bound

```
$ ./target/debug/slither-demo --scenario partition --seed 5 | tail -4
{"seq":149,"t_us":24934000,…,"fate":"blackholed"}
{"seq":150,"t_us":25000000,"kind":"lost","side":"a",
 "cause":"no authenticated packet arrived for DEAD_TIMEOUT","silent_us":25000000}
{"seq":151,"t_us":25000000,"kind":"lost","side":"b",
 "cause":"no authenticated packet arrived for DEAD_TIMEOUT"}
{"seq":152,"t_us":25000000,"kind":"end","ok":true,"wall_us":38907,
 "sent":134,"delivered":10,"lost":0,"duplicated":0,"blackholed":124}
```

**25 000 000 µs on the nose.** 124 datagrams sent into nothing, every
`send_to` returning `Ok`, and then the connection dies with no packet
having announced it. 38.9 ms of wall time for 25 s of protocol.

### Two defects found by running it, both fixed

1. **`end` was stamped with wall time.** It was emitted after
   `LocalSet::block_on` returned — i.e. outside the runtime, where
   `tokio::time::Instant::now()` falls back to the real clock. A clean run
   whose every event was at `t_us` 0 ended at `t_us` 56472, which is
   *plausible enough to ship*. `end` is now emitted inside `block_on`.
2. **`pkt` ids collided.** They were per-wire, so `a`'s packet 1 and `b`'s
   packet 1 were different datagrams and any animation keyed on the id
   alone would draw one dot for two packets. Numbering moved to the log.

### A degenerate case worth naming for the UI

On a clean, delay-free run **every event is at `t_us` 0** — correctly:
virtual time only advances when something waits, and nothing does. So the
time axis must fall back to `seq` ordering when the run's span is zero,
and must never present `t_us` as "how long this took in your browser".
`app.js` does this and the page says so.

---

## Verification runs — wasm

### Under `node:wasi` (Node 22.22.2)

`demo/verify/run-node-wasi.mjs`. The lossy run, byte for byte against the
native one:

```
$ node demo/verify/run-node-wasi.mjs -- --scenario lossy --loss 0.35 --seed 6
{"seq":3,"t_us":0,…,"type":"init","fate":"lost"}
{"seq":4,"t_us":5034000,…,"type":"init","fate":"lost"}
{"seq":5,"t_us":10113000,…,"type":"init","fate":"delivered","arrive_us":10113000}
{"seq":39,"t_us":14359000,"kind":"end","ok":true,"wall_us":13343,"sent":22,…}
```

**Every `seq`, every `t_us`, every fate identical to the native run.** The
demo is deterministic across targets — the seed is the whole state.

### Through the **vendored** shim, via `wasm-run.js` — the file the Worker uses

`demo/verify/run-vendored-shim.mjs` imports the same `demo/web/wasm-run.js`
the browser Worker imports, so what is checked here is what ships.

```
$ node demo/verify/run-vendored-shim.mjs

clean: slither-demo --scenario clean --seed 12648430
  ok    29 lines, all JSON
  ok    exit code 0
  ok    end ok — 11 sent, 11 delivered, 0 lost, 0 duplicated, 0 blackholed
  ok    seq is dense from 0
  ok    t_us is monotonic
  ok    poll_oneoff: 0 calls
  ok    0 ms of protocol time in 15 ms of wall time
  ok    every event carries the fields app.js reads (9 kinds seen)

lossy: slither-demo --scenario lossy --seed 6 --loss 0.35
  … ok    14359 ms of protocol time in 6 ms of wall time
  ok    msg1 was dropped at least once
  ok    the retransmit ladder advanced virtual time past 5 s

duplicate: slither-demo --scenario duplicate --seed 3 --duplicate 0.6
  ok    end ok — 23 sent, 38 delivered, 0 lost, 15 duplicated, 0 blackholed
  ok    a duplicate copy was delivered
  ok    the replay window rejected one

partition: slither-demo --scenario partition --seed 5
  ok    end ok — 134 sent, 10 delivered, 0 lost, 0 duplicated, 124 blackholed
  ok    25000 ms of protocol time in 5 ms of wall time
  ok    datagrams were blackholed
  ok    both sides died on the liveness bound

ALL CHECKS PASSED
```

**`poll_oneoff: 0 calls` on all four** — the counter wraps the import
before instantiation. This is the feasibility report's browser-risk claim
turned into an assertion instead of an argument, and the vendored shim's
source says why it matters: `poll_oneoff` there is
`while(endTime>getNow()){}`, so one call for a 25-second timeout is a
25-second frozen core.

The field-shape check earns its place by failing when it should. Injecting
a required field the runner does not emit:

```
$ (temporarily added "arrival_time_that_does_not_exist" to REQUIRED.tx)
  FAIL  fields the page needs are missing: tx.arrival_time_that_does_not_exist   (×4)
  4 CHECK(S) FAILED
```

### The page's own logic, executed

`demo/verify/render-smoke.mjs` boots the real `app.js` against a stub DOM
and drives 300 animation frames over a real timeline. The Worker stub runs
`runScenario` in process and speaks `worker.js`'s exact message protocol,
so this also pins the `app.js` ↔ `worker.js` contract, which nothing else
checks.

```
$ node demo/verify/render-smoke.mjs
  ok    300 animation frames, no exception
  ok    45392 canvas operations issued
  ok    29 log rows rendered
  ok    counter "datagrams sent" reached 11
  ok    DH counter reads 4 / 4
  ok    staged-accept badges lit: dial, established, intro, claimed, proven, connection
  ok    status line: "29 events · 12 ms of your CPU · the runtime never waited on a real clock"
switching scenario to lossy
  ok    200 further frames after the scenario switch
  ok    handshake retries counter reached 2 on the lossy run
ALL CHECKS PASSED
```

It earned its keep immediately: its first run failed on
`document.createTextNode is not a function` — a gap in the stub, not in
the page, but the same harness is what would catch the inverse. Its
scenario-switch check also shipped, briefly, as `pass(\`${more} frames\`)`,
which reports whatever number it finds and therefore passes on zero — the
exact failure it exists to catch. Now `more > 50`.

### The delayed-delivery path, which nothing above exercised

`ObservedWire` schedules delayed copies with `spawn_local`. Checked
separately, because a 0 ms delay never reaches that branch:

```
$ node demo/verify/run-node-wasi.mjs -- --scenario clean --delay-ms 40 --jitter-ms 30 --seed 11
{"seq":3,"t_us":0,…,"type":"init","fate":"delivered","arrive_us":66333}
{"seq":4,"t_us":67000,"kind":"stage","side":"b","stage":"intro",…}
{"seq":7,"t_us":67000,…,"type":"resp","fate":"delivered","arrive_us":127273}
{"seq":9,"t_us":128000,"kind":"stage","side":"a","stage":"established",…}
```

msg1 leaves at 0 claiming it will arrive at 66.333 ms; `b`'s `intro` fires
at 67 ms. msg2 leaves at 67 ms claiming 127.273 ms; `a`'s `established` is
at 128 ms. **The claimed arrival times are corroborated by when the
protocol actually reacts**, to within tokio's 1 ms virtual timer
granularity — so `arrive_us` is a measurement, not a decoration.

### Argument handling and hostile input

```
$ … --scenario nope   → {"kind":"end","ok":false,"error":"no such scenario: nope"}   exit 1
$ … --loss 5          → {"kind":"error","message":"bad loss: 5 is outside [0, 1]"}   exit 2
$ … --bogus x         → {"kind":"error","message":"unknown argument: --bogus"}       exit 2
$ … --message 'quote " backslash \ tab<TAB> ünïcode 🐍 <script>'
run  "quote \" backslash \\ tab\t ünïcode 🐍 <script>"
app  "quote \" backslash \\ tab\t ünïcode 🐍 <script>"
app  "ack: quote \" backslash \\ tab\t ünïcode 🐍 <script>"
```

Round-trips through argv, the hand-rolled JSON writer, `JSON.parse`, and
the protocol's stream. `grep -n 'innerHTML|outerHTML|insertAdjacentHTML|eval('
demo/web/*.js` returns nothing — the page builds every node with
`textContent`, so the `<script>` above is inert by construction rather
than by escaping.

### The workflow's exact sequence, run locally

`.github/workflows/pages.yml` cannot run here — there is no remote and no
Pages environment, so `actions/configure-pages`, `upload-pages-artifact`
and `deploy-pages` are **unverified**. Everything before them is not:

```
$ (cd demo/runner; cargo test)                  → 8 passed
$ (cd demo/runner; cargo build --release --target wasm32-wasip1) → Finished
$ node demo/verify/check-web.mjs                → ALL CHECKS PASSED
$ node demo/verify/run-vendored-shim.mjs        → ALL CHECKS PASSED
$ node demo/verify/render-smoke.mjs             → ALL CHECKS PASSED
$ ./demo/build.sh                               → _site ready (628 KB of wasm)
```

The YAML itself parses, and its structure is what it claims:

```
$ python3 -c "import yaml; d=yaml.safe_load(open('.github/workflows/pages.yml')); …"
parsed OK
name: Pages
jobs: ['build', 'deploy']
triggers: ['push', 'workflow_dispatch']
permissions: {'contents': 'read', 'pages': 'write', 'id-token': 'write'}
missing files referenced by run: steps: none
```

## Main-crate gates — unaffected

Only `Cargo.toml`'s `exclude` list was touched (one entry, five lines with
its comment). Nothing in `src/`, `README.md` or `CHANGELOG.md`.

```
$ cargo package --list --allow-dirty | head -5
.cargo_vcs_info.json
CHANGELOG.md
Cargo.lock
Cargo.toml
Cargo.toml.orig

$ cargo package --list --allow-dirty | wc -l        → 115
$ cargo package --list --allow-dirty | grep -c demo → 0
```

(`--allow-dirty` because the tree is deliberately left uncommitted for the
integrator; without it cargo refuses on the modified `Cargo.toml`.)

**The exclude entry is load-bearing, checked rather than assumed.** With
`"/demo"` removed, cargo packages **22 demo files** — `web/`, `verify/`,
`build.sh` and the whole vendored shim. It skips `demo/runner/` on its own
(a subdirectory with its own manifest), which is exactly the sort of
partial behaviour that makes "cargo probably handles it" wrong.

```
$ cargo build --all-features --all-targets                    → Finished
$ cargo fmt --all --check                                     → no diff
$ cargo clippy --all-features --all-targets -- -D warnings    → zero warnings
$ RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features → Generated, exit 0
$ RUSTDOCFLAGS=-D warnings cargo doc --no-deps                → Generated, exit 0
$ cargo test --all-features                                   → 1138 passed, 0 failed
$ cargo deny check                       → advisories ok, bans ok, licenses ok, sources ok
```

The runner's own gates:

```
$ cd demo/runner; cargo fmt --check                              → no diff
$ cd demo/runner; cargo clippy --all-targets -- -D warnings      → zero warnings
$ cd demo/runner; cargo clippy --target wasm32-wasip1 -- -D warnings → zero warnings
$ cd demo/runner; cargo test                                     → 8 passed
```

Clippy found one real thing: `emit` took eight positional arguments, two of
them adjacent `&str`s (`kind` and `fate`) whose transposition is not a type
error. Replaced with a `Tx` struct.

## Findings for the integrator (reported, not acted on — rules 3 and 5)

**1. `Wire` is the only item on the primary integration path not
re-exported at the crate root.** It lives at `slither::shell::wire::Wire`
(`src/shell/wire.rs:70`); `lib.rs` re-exports `Connection`, `Endpoint`,
`EndpointBuilder`, `Intro`, `Claimed`, `Proven`, `Connecting`, `BiStream`,
`SendStream`, `RecvStream`, `Notification`, `Config`, `WallClock`, every
error type and `SessionId` — but not `Wire`, the one trait a consumer with
its own socket **must** implement. `lib.rs`'s own prose refers to it as
`shell::wire::Wire` in three places (lines 160, 315), so this looks
deliberate rather than overlooked; recording it because writing the demo is
the first time anyone has implemented `Wire` from outside the crate, and
the first thing that happened was a compile error and a `grep`. Not a bug,
and not mine to change.

**2. The brief's DH figure is wrong, and the code is right.** The brief
says the staged ladder is "0→1→2". Measured: the initiator goes 0→2 across
msg1 and 2→4 across msg2, and the responder 0→1→2→4 across the four rungs.
§6.1 prices the ladder cumulatively and the initiator pays two at a time.
The demo reports both counters at every rung rather than asserting a shape,
so nothing here depends on which is right — but the number in the brief
should not be repeated in release copy.

**3. `browser_wasi_shim` turns its syscall logger ON when you omit the
options object.** `debug.enable(options.debug)` with `options.debug ===
undefined` resolves to `true` (`enabled === undefined ? true : enabled`).
The verifier printed `wasi: 5 46` per run before `{ debug: false }` was
passed explicitly. Recorded in `wasm-run.js` beside the fix, because the
next person to construct a `WASI` here will otherwise reintroduce it.

## Deviations from the brief

1. **Not `slither::block_on`.** `src/compat/rt.rs:75` builds
   `new_current_thread().enable_all()` with no `start_paused`, and the
   brief also mandates the paused clock. The two are not simultaneously
   satisfiable, so the runner builds the four-line runtime itself — which
   is what the feasibility report's own paused-clock proof did. The shell
   being driven is identical either way.
2. **The demo owns the faults, not `FlakyPolicy`.** Explained at length
   under Architecture: the tap cannot answer "did this arrive", and the
   brief asks for dropped packets "visibly dying mid-flight". The
   in-memory `Network` still does all routing and delivery; only the
   loss / duplication / delay / blackhole decisions moved into
   `demo/runner/src/wire.rs`, behind the crate's public `Wire` trait.
   Consequence: `Network::tap()` and `Network::sends()` go unused, and
   `Network::partition()` is replaced by a demo-side flag whose state the
   demo can actually read.
3. **All four scenarios, not three.** Partition (stretch #4) works and
   lands on `DEAD_TIMEOUT` at exactly 25 000 000 µs, so it is in. Stretch
   #5 (custom message) is not a separate scenario: `--message` is a
   parameter of every scenario, which is strictly more than was asked.
4. **Four verification harnesses, not two.** The brief asked for node:wasi
   and the vendored shim. `check-web.mjs` and `render-smoke.mjs` were
   added because "the JavaScript parses" is a far weaker claim than "the
   JavaScript runs", and no browser is available here.

## Remaining work

**Unverified, and only a browser settles it:**

- The page in an actual browser: module Worker construction, `fetch` of
  the `.wasm` from a Pages origin, canvas rendering, whether anything is
  legible, whether the dark theme reads correctly, small-screen layout.
  Everything in `verify/` exercises the *logic*; none of it produces a
  pixel.
- `.github/workflows/pages.yml`'s last three steps (`configure-pages`,
  `upload-pages-artifact`, `deploy-pages`) — no remote exists here. Every
  step before them ran locally.
- Pages must be enabled for the repository with **Source = GitHub
  Actions**, and the `github-pages` environment must exist, or the deploy
  job fails on its first run.

**Left deliberately for the integrator:**

- `demo/web/index.html` carries a placeholder repo link:
  `https://github.com/REPLACE-ME/slither`. `Cargo.toml` says the real one
  is `primetype/slither`; not hard-coded because the demo may be published
  from a fork or a mirror, and a wrong link is worse than an obvious
  placeholder. **`check-web.mjs` will not catch this** — it allow-lists
  any `github.com` URL in the page's prose.
- README / CHANGELOG mention of the demo, and a link to the deployed page.
  Out of scope per the brief.
- Whether the demo deserves a `docs/` diagram or a line in `SPEC.md`'s
  front matter. It deserves neither in my view — it is not protocol.

**Possible next increments, in rough value order:**

1. **Roaming.** `Peer::rebind` exists, §7.3's receive-driven re-home is one
   of slither's better stories, and a sequence diagram is exactly the right
   shape to show a lifeline changing address mid-run. The wire wrapper
   would need to follow a second address, which is why it is not here.
2. **Send failure.** Ruling 49's `ENETUNREACH` injection is a few lines in
   `ObservedWire`, and it is the case working rule 13 records as having
   been *unreachable by construction* until the fixture grew it — worth
   showing precisely because it looks like a partition and is not.
3. `wasm-opt -Oz` in the workflow: 628 KB is already fine, but halving it
   is nearly free.
4. A URL-parameter permalink (`?scenario=lossy&loss=0.35&seed=6`). The runs
   are fully determined by the seed, so a link reproduces a run exactly —
   cheap, and the single best thing for sharing one specific behaviour.

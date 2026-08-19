# Release prep — can slither power a browser demo on GitHub Pages?

A measurement report. No protocol behaviour is changed; every Cargo.toml
edit in this worktree is a throwaway probe.

## Base

- Worktree: `.claude/worktrees/agent-a45a2a50f7391112f`
- `git log -1 --oneline`: `27c8632 Round 44 follow-up 2: the suite declaration is visible in the quickstart`
- `git status --porcelain`: empty (clean)
- Expected base was `27c8632`. MATCH. Tree clean. (Rule 14 discharged.)

## Inventory

### 1. slither's own tokio features — `net` is unconditional

`Cargo.toml`, `[dependencies]`:

```toml
tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
```

`net` is a **hard, unconditional, non-optional** feature of the library
itself, not a dev-dependency and not target-gated. `tokio/net` pulls
`mio` + `socket2` on every target. This is blocker #1.

Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1`.

### 2. Clock call sites — the core invariant holds exactly

`grep -rn 'Instant::now' src/` returns sites in
`src/core/connection/{session,mobility,close,mod}.rs` and
`src/core/connection/timers.rs`, but **every one is inside that file's
`#[cfg(test)]` module**:

| file | `#[cfg(test)]` module opens at | earliest `Instant::now()` |
|------|------|------|
| `src/core/connection/session.rs` | 636, 865 | 777 |
| `src/core/connection/mobility.rs` | 297 | 405 |
| `src/core/connection/close.rs` | 137 | 149 |
| `src/core/connection/mod.rs` | 3799 | 4001 |
| `src/core/connection/timers.rs` | (test mod) | 223 (`fn t0()`) |

Plus `src/core/tests.rs:297`, `src/core/endpoint/tests.rs:317`,
`src/core/connection/testfix.rs:467` — all test fixtures.

**There is no non-test clock read anywhere in `src/core/`.** The
architecture invariant is real and measured, not aspirational. This is
what makes shape B conceivable at all.

Production clock reads, all in the shell:

```
src/shell/shared.rs:40:pub(crate) fn now() -> std::time::Instant {
src/shell/shared.rs:41:    tokio::time::Instant::now().into_std()
src/shell/mod.rs:433:            let started = tokio::time::Instant::now();
src/shell/driver.rs:1521:  tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await
```

Note `src/shell/shared.rs:40` — slither already mints a
`std::time::Instant` **from a `tokio::time::Instant`** via `into_std()`.
Relevant to the minting question (§3f) but not a rescue: see below.

### 3. Wall clock — already injectable

`src/config.rs:32-52`:

```rust
pub trait WallClock {
    fn now(&self) -> Timestamp;
}
pub struct SystemClock;
impl WallClock for SystemClock {
    fn now(&self) -> Timestamp {
        match SystemTime::now().duration_since(UNIX_EPOCH) { ... }
    }
}
```

`SystemTime::now()` also panics on `wasm32-unknown-unknown`, but it is
behind a **public injectable trait**, so a demo supplies its own
`WallClock` and never constructs `SystemClock`. **Not a blocker** —
already designed for this ("so an embedded host can supply whatever time
source it has").

### 4. `tokio::net::UdpSocket` Wire impl — NOT cfg-gated

`src/shell/wire.rs:76-83`:

```rust
impl Wire for tokio::net::UdpSocket {
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        tokio::net::UdpSocket::send_to(self, buf, addr).await
    }
    ...
}
```

No `#[cfg(...)]`, no feature gate. This is the only *code* site that
needs `tokio/net`; everything else is doc-comments and one `#[cfg(test)]`
round-trip test at `src/shell/wire.rs:97`.

### 5. `testutil` / `FlakyWire` — feature-gated, no `net`

`src/lib.rs:361`:

```rust
#[cfg(any(test, feature = "test-util"))]
pub mod testutil;
```

`src/testutil/mod.rs` uses only `tokio::sync::Notify`,
`tokio::time::{Instant, sleep_until}`, `tokio::task::LocalSet`,
`tokio::select!`, `tokio::pin!` — i.e. `sync` + `time` + `rt` + `macros`.
**No `tokio::net` at all** (the one mention at line 841 is a doc comment
comparing semantics). `Cargo.toml`'s own comment confirms it:
*"Dependency-free — `FlakyWire` needs only `tokio/time` and
`tokio/sync`, both hard deps."* Correct as measured.

### 6. Randomness — `getrandom` is a hard library dependency

```toml
getrandom = "0.4"
```

Used in production at `src/shell/endpoint.rs:575`:

```rust
getrandom::fill(&mut seed).expect("OS entropy for the endpoint RNG (§16.6)");
```

`rand_chacha`/`rand_core` 0.10 are pure-Rust and portable. `getrandom`
0.4 on `wasm32-unknown-unknown` has **no default backend** and requires
an explicit `--cfg getrandom_backend="wasm_js"` plus a `getrandom`
feature. This is blocker #3 (soft — it has a documented escape hatch,
and the call site is in the *shell*, not the core).


## Target experiments

`rustup target list --installed` had `wasm32-unknown-unknown` already;
`wasm32-wasip1` was missing and was added (`rustup target add
wasm32-wasip1` → `info: downloading component rust-std`, both present
afterwards).

### (a) `cargo check --target wasm32-unknown-unknown` — FAILS, 2 root causes

```
error: This wasm target is unsupported by mio. If using Tokio, disable the net feature.
  --> mio-1.2.2/src/lib.rs:44:1
   |
44 | compile_error!("This wasm target is unsupported by mio. If using Tokio, disable the net feature.");

error: The wasm32/64-unknown-unknown are not supported by default; you may need to enable the "wasm_js" crate feature.
  --> getrandom-0.4.3/src/backends.rs:176:17
```

Followed by ~48 cascade errors inside `mio` (`unresolved import
crate::sys::IoSourceState`, `could not find tcp in sys`, `cannot find
Selector in sys`, `no method named register/reregister/deregister found
for struct IoSource<std::net::UdpSocket>`, and an `expected UdpSocket,
found ()` at `mio/src/net/udp.rs:768`) — all downstream of the same
thing: mio has no `sys` backend for wasm, so its whole platform layer
resolves to nothing. `error: could not compile mio (lib) due to 48
previous errors`.

**Root cause 1: `tokio/net` → `mio`.** Not slither's code — slither's
own source never got compiled; the build died in a transitive dependency
pulled in *only* by the `net` feature.

**Root cause 2: `getrandom` 0.4 has no wasm32-unknown-unknown backend by
default** (that target has no OS entropy syscall; the browser's
`crypto.getRandomValues` has to be reached through JS).

### (b) `--target wasm32-unknown-unknown --features test-util` — FAILS, identical

Same two `compile_error!`s, same mio cascade. `test-util` adds **nothing**
to the blocker set — consistent with the inventory finding that
`testutil` uses no `tokio::net`.

### (c) `cargo check --target wasm32-wasip1` — FAILS, ONE root cause

```
error: Only features sync,macros,io-util,rt,time are supported on wasm.
   --> tokio-1.53.1/src/lib.rs:479:1
    |
479 | compile_error!("Only features sync,macros,io-util,rt,time are supported on wasm.");

error: could not compile `tokio` (lib) due to 1 previous error
```

**This is the decisive measurement.** Note what compiled *cleanly* on the
way there: `mio v1.2.2` — checked, no error (mio does have a wasi
backend) — plus `getrandom v0.4.3`, `cryptoxide`, `eccoxide`,
`rand_core`, `rand_chacha`, `packtool`, `tracing`, `wasi`. The **only**
failure is tokio's own hand-written wasm feature gate, which refuses
`net` on *any* wasm target regardless of whether mio could handle it.

This also settles the step-3g question about tokio's wasm support matrix
**from the source rather than from memory**: the supported set is
literally `sync, macros, io-util, rt, time`. `rt` and `time` — the two
the shell actually needs — are both in it. `net` is not, and never will
be.

### (d) `--target wasm32-wasip1 --features test-util` — FAILS, identical

Same single `compile_error!`. `hiss v0.3.2` checked clean immediately
before it — **the entire crypto stack builds on wasip1**.

### (e) The probe: target-gate `tokio/net`

Exact diff applied in this worktree (throwaway):

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
-tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
+tokio = { version = "1", features = ["rt", "time", "sync", "macros"] }

+# ── THROWAWAY PROBE (wasm measurement, NOT a patch) ──────────────────────
+[target."cfg(not(target_family = \"wasm\"))".dependencies]
+tokio = { version = "1", features = ["net"] }
```

```diff
--- a/src/shell/wire.rs
+++ b/src/shell/wire.rs
+// THROWAWAY PROBE (wasm measurement) — not a patch.
+#[cfg(not(target_family = "wasm"))]
 impl Wire for tokio::net::UdpSocket {
```

That is the **whole** diff — two hunks. Cargo unions the base and
target-specific feature sets, so every non-wasm target keeps `net`
exactly as before and nothing about the native build changes.

Re-running (c) and (d):

```
$ cargo check --target wasm32-wasip1
    Checking tokio v1.53.1
    Checking slither v0.2.0 (.../agent-a45a2a50f7391112f)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.98s

$ cargo check --target wasm32-wasip1 --features test-util
    Checking slither v0.2.0 (.../agent-a45a2a50f7391112f)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.58s
```

**slither compiles clean on `wasm32-wasip1`, with `test-util`, on a
two-hunk diff.** No source change beyond one `#[cfg]`.

Re-running (b):

```
$ cargo check --target wasm32-unknown-unknown --features test-util
error: The wasm32/64-unknown-unknown are not supported by default; you may need
       to enable the "wasm_js" crate feature.
   --> getrandom-0.4.3/src/backends.rs:176:17
error: could not compile `getrandom` (lib) due to 1 previous error
```

mio is gone; **tokio itself checked clean on wasm32-unknown-unknown**
with `rt,time,sync,macros`. One blocker left. Adding the documented
escape hatch as a third probe hunk:

```diff
-getrandom = "0.4"
+getrandom = { version = "0.4", features = ["wasm_js"] } # THROWAWAY PROBE
```

```
$ RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
      cargo check --target wasm32-unknown-unknown --features test-util
    Checking hiss v0.3.2
   Compiling wasm-bindgen-macro v0.2.127
    Checking slither v0.2.0 (.../agent-a45a2a50f7391112f)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.64s
```

**Both wasm targets compile clean.** Note `wasm_js` pulls
`wasm-bindgen 0.2.127` into the graph on that target — acceptable for a
browser demo, but it is a real dependency-graph change and it needs the
`RUSTFLAGS` cfg as well as the crate feature (the feature alone is not
enough; getrandom 0.4 requires both).

### Summary table

| target | features | before probe | after probe |
|---|---|---|---|
| `wasm32-unknown-unknown` | default | mio + getrandom | getrandom only → **clean** w/ `wasm_js` + RUSTFLAGS |
| `wasm32-unknown-unknown` | `test-util` | mio + getrandom | **clean** w/ `wasm_js` + RUSTFLAGS |
| `wasm32-wasip1` | default | `tokio/net` gate | **clean** |
| `wasm32-wasip1` | `test-util` | `tokio/net` gate | **clean** |

### (f) KEY QUESTION — can `Instant` be minted on wasm32-unknown-unknown?

**Answer: `Instant::now()` traps, but an `Instant` CAN be minted, and all
the arithmetic the cores need works. Shape B is not dead — but the only
seed is three lines of `unsafe` that depend on an unstable std internal.**

Answered two ways: from std's source, and by **executing wasm under
Node**.

*From the source* — `library/std/src/sys/time/mod.rs` dispatches:

```rust
cfg_select! {
    ...
    any(
        target_os = "teeos",
        target_family = "unix",
        target_os = "wasi",
    ) => {
        mod unix;
        use unix as imp;
    }
    ...
    _ => {
        mod unsupported;
        use unsupported as imp;
    }
}
```

- `wasm32-wasip1` sets `target_os = "wasi"` → the **`unix` backend, a
  real clock**. `Instant::now()` works there. (This is the source-level
  confirmation of "WASI has clocks", not a from-memory claim.)
- `wasm32-unknown-unknown` matches no arm → `_` → `unsupported.rs`:

```rust
pub struct Instant(Duration);
impl Instant {
    pub fn now() -> Instant {
        panic!("time not implemented on this platform")
    }
    pub fn checked_sub_instant(&self, other: &Instant) -> Option<Duration> { ... }
    pub fn checked_add_duration(&self, other: &Duration) -> Option<Instant> { ... }
    pub fn checked_sub_duration(&self, other: &Duration) -> Option<Instant> { ... }
}
```

Note what this says: **`now()` is the only thing that panics.** Every
arithmetic and comparison operation is pure `Duration` math and is fully
implemented. `std::time::Instant` has no public constructor, so the
question reduces to: can you obtain one seed value?

*By execution.* A `cdylib` built for `wasm32-unknown-unknown` and
instantiated in Node 22.22.2 with no imports:

```rust
#[unsafe(no_mangle)] pub extern "C" fn probe_now() -> u32 {
    let a = Instant::now(); let b = a + Duration::from_millis(5);
    if b > a { 1 } else { 2 }
}
#[unsafe(no_mangle)] pub extern "C" fn probe_minted() -> u32 {
    let t0: Instant = unsafe { std::mem::zeroed() };
    let t1 = t0 + Duration::from_millis(250);
    let ord = t1 > t0;
    let d = t1.duration_since(t0);
    let t2 = t1.checked_add(Duration::from_secs(1)).unwrap();
    let back = t2.checked_sub(Duration::from_secs(1)).unwrap();
    if ord && back == t1 { d.as_millis() as u32 } else { 0 }
}
#[unsafe(no_mangle)] pub extern "C" fn probe_saturating() -> u32 {
    let t0: Instant = unsafe { std::mem::zeroed() };
    let t1 = t0 + Duration::from_millis(40);
    t0.saturating_duration_since(t1).as_millis() as u32
}
```

```
$ cargo build --release --target wasm32-unknown-unknown
    Finished `release` profile [optimized] target(s) in 0.23s
$ node run.mjs
probe_now: TRAPPED -> RuntimeError: unreachable
probe_minted: RETURNED 250
probe_saturating: RETURNED 0
```

Read that carefully:

1. `Instant::now()` **traps** — the `panic!` lowers to `unreachable`.
   Confirmed by execution, not inferred.
2. A **minted** `Instant` works perfectly: ordering, `duration_since`,
   `checked_add`, `checked_sub`, round-trip equality, and
   `saturating_duration_since` (returns 0 rather than panicking on the
   negative direction — which slither's timer logic relies on).

**Soundness of the seed.** `mem::zeroed::<Instant>()` is sound *today*
because the `unsupported` backend's `Instant` is a newtype over
`Duration`, and `Duration` is `{ secs: u64, nanos: Nanoseconds }` with
the documented invariant `// Always 0 <= nanos < NANOS_PER_SEC`. Zero
satisfies it, and `Duration` derives `Default`. But this is a **claim
about a private std internal**: if the `unsupported` `Instant` ever gains
a niche or a non-zero representation, the demo becomes UB silently. It
is acceptable in a demo crate; it must **never** go anywhere near
slither itself.

*(Aside, checked so it is not later proposed as the fix: the `web-time`
crate does not rescue this. `web_time::Instant` is its own type on wasm,
not `std::time::Instant`, so it cannot be handed to a core API whose
signature says `std::time::Instant`.)*

## Blockers

Ranked, with the measured root cause for each.

| # | Blocker | Target | Root cause | Fix cost |
|---|---|---|---|---|
| 1 | `tokio/net` → `mio` | both | `mio` has no wasm `sys` backend (`compile_error!` + 48 cascade); tokio *also* refuses `net` on wasm by its own gate | **2-hunk diff**, measured green |
| 2 | `getrandom` 0.4 no default backend | unknown-unknown **only** | no OS entropy syscall on that target | crate feature `wasm_js` **plus** `RUSTFLAGS --cfg getrandom_backend="wasm_js"`; pulls `wasm-bindgen` |
| 3 | `Instant::now()` panics | unknown-unknown **only** | std `sys/time/unsupported.rs` | not fixable; **workaroundable** in the demo via a minted seed (measured) |
| 4 | `SystemTime::now()` panics | unknown-unknown **only** | same file | **already solved** — `WallClock` is a public injectable trait; never construct `SystemClock` |
| 5 | `block_on` blocks the main thread | both, runtime | browsers forbid blocking the UI thread | run in a **Web Worker** |

Blocker 1 is the only one that touches slither. Blockers 2–4 are
consumer-side on unknown-unknown and **do not exist at all on wasip1**.

### (g) Shape A, PROVEN BY EXECUTION — not by argument

The brief asked me to "report what compiles and what is known to work per
tokio's documented wasm support matrix, cited from memory". I did better:
**I built the real thing and ran it.** Nothing below is from memory.

A binary crate depending on this worktree's slither by path, using the
**real shell** — `slither::block_on`, the `spawn_local` driver,
`Endpoint`/`Connection` handles, `testutil::Pair` — built for
`wasm32-wasip1` and executed under **Node 22.22.2's `node:wasi`**
(WASI preview1, the same ABI `@bjorn3/browser_wasi_shim` implements):

```
$ cargo build --release --target wasm32-wasip1
   Compiling slither v0.2.0 (.../agent-a45a2a50f7391112f)
   Compiling wasidemo v0.1.0 (...)
    Finished `release` profile [optimized] target(s) in 7.23s

$ node --experimental-wasi-unstable-preview1 run.mjs
wasidemo: starting
wasidemo: Instant::now() OK
wasidemo: tokio::time::sleep(250ms) returned after 252.907333ms
wasidemo: handshake complete, sends=2
wasidemo: wrote 15 bytes; peer read Some("hello from wasm")
wasidemo: handshake through drop_first(2) took 10.108426292s, sends=4
wasidemo: total elapsed 10.399148166s
wasidemo: OK
```

Every line of that is a measurement:

1. **`Instant::now()` works** on wasip1 — the WASI clock, as std's dispatch
   predicted.
2. **tokio's timers actually fire.** `sleep(250ms)` returned after
   252.9 ms, so the current-thread runtime **parks and wakes correctly**
   under a WASI host. This was the real risk and it is retired.
3. **The full IK handshake completes** over `FlakyWire` — 2 datagrams,
   4 DHs a side.
4. **The frame layer works**: a bi stream carried 15 bytes and the peer
   read them back.
5. **Loss and retransmission work in real time**: through
   `FlakyPolicy::drop_first(2)` the handshake took **10.1 seconds** —
   that is the ratified WireGuard retransmit ladder (two 5 s
   `REKEY_TIMEOUT` expiries) firing on a real clock, inside wasm.

**The entire protocol runs in WebAssembly with no kernel networking.**

### (g2) The paused clock works under WASI — the demo's fast-forward

10.1 seconds is correct protocol behaviour and terrible demo UX, and the
timers are frozen so they cannot be shortened. But slither's own test
methodology solves it, and it works on wasm:

```rust
let rt = tokio::runtime::Builder::new_current_thread()
    .enable_time()
    .start_paused(true)      // virtual time
    .build().unwrap();
tokio::task::LocalSet::new().block_on(&rt, async { ... });
```

```
$ node --experimental-wasi-unstable-preview1 run.mjs
wasidemo2: virtual sleep(30s) cost 242.458µs of WALL time
wasidemo2: handshake through drop_first(2): 9.7845ms WALL, sends=4
wasidemo2: OK
wasidemo2: total wall time 12.8075ms
```

The **same** lossy handshake that cost 10.1 s of wall time completes in
**9.8 ms**. `tokio/test-util` is not in tokio's refused-on-wasm list, and
it compiles and runs on wasip1.

Two consequences, and the second is the important one:

- The demo can **scrub the protocol's timers** — a "fast-forward"
  control that steps a 5-second retransmit ladder instantly. This is the
  single best thing about the demo, and it is free.
- **It removes shape A's last browser risk.** Under `start_paused(true)`
  tokio auto-advances virtual time when the runtime goes idle and
  **never parks on a real timer**, so the browser shim's `poll_oneoff`
  — the syscall a JS WASI shim is least likely to implement well, since
  a browser cannot block synchronously — is never exercised for time.
  What remains is `clock_time_get`, `fd_write` and `random_get`, all
  trivial in any shim.

**Honest scope of this proof:** I verified the preview1 ABI under
`node:wasi`. I did **not** run `@bjorn3/browser_wasi_shim` in a real
browser — there is no browser in this environment. That is the one
remaining unverified step in shape A, and the paused-clock result is
what makes me confident it is a small one.

## Demo shapes A / B / C

### Shape A — wasip1 + browser WASI shim, real shell in a Web Worker

**Verdict: RECOMMENDED. Proven to the ABI boundary. Effort M — 3–5 agent-days.**

Works because everything hard is already measured green: the crate
compiles on wasip1 with `test-util` on a two-hunk diff, the real shell
runs, timers fire, loss and retransmission behave, and virtual time makes
it interactive.

Architecture:

- Rust `demo/` crate → `wasm32-wasip1`, using `testutil::Pair` /
  `Network` as the backend.
- Runs **in a Web Worker**. `block_on` blocks its thread for the whole
  run, which would freeze the UI on the main thread. In a Worker that is
  harmless.
- The worker exports host functions the wasm calls to emit events;
  those `postMessage` to the UI thread. Posting from inside a blocking
  `block_on` is fine — `postMessage` queues rather than blocks.
- Visualisation data is already there: `Network::tap()` →
  `Tap::datagrams()` yields `(src, dst, bytes)` per datagram —
  the packet-flow animation, for free. `net.sends()`, `peer.dhs.get()`,
  `peer.addr()` (roaming), and §18.2's five `tracing` targets feed the
  side panels.

What slither itself needs — **all of it is item 1 below, and nothing else**:

| change | touches public API? | touches ratified behaviour? |
|---|---|---|
| `Cargo.toml`: target-gate `tokio/net` | **no** | **no** |
| `src/shell/wire.rs`: `#[cfg(not(target_family = "wasm"))]` on the `impl Wire for tokio::net::UdpSocket` | **yes, on wasm targets only** — the impl vanishes there. On every native target the surface is byte-identical | **no** |

No `getrandom` change (wasip1 has a backend), no wasm feature, no
`WallClock` change, no new dependency, no core promotion.

### Shape B — wasm32-unknown-unknown, no tokio, JS-tick driver over the cores

**Verdict: NOT VIABLE as briefed. Blocked on a public-API decision the
maintainer explicitly deferred. Effort L — 8–12 agent-days, and it
duplicates `src/shell/driver.rs`.**

The brief asks: *"is the core API public enough for this?"* Measured
answer: **no, and not by a small margin.**

`src/lib.rs:359`:

```rust
pub(crate) mod core;
```

`src/core/mod.rs:74,89`:

```rust
pub(crate) use self::connection::{ConnEvent, ConnOutput, Connection, StreamRef, StreamsExhausted};
pub(crate) use self::endpoint::Endpoint;
```

`core::Endpoint` and `core::Connection` — the two things shape B is
defined as driving — are **`pub(crate)`**. `poll_output()`,
`ConnOutput`, `ConnEvent` and `TimerKind` are all unreachable from
outside the crate. Only `Dir`, `StreamId`, `IntroId` and `Timestamp` are
re-exported.

So shape B cannot begin without promoting the core surface to `pub`, and
`src/lib.rs:348` records that this was a **deliberate decision with a
stated reason**:

> `pub(crate)` in this slice, deliberately: nothing outside the crate can
> drive them until the driver lands, so publishing the surface now would
> freeze an unusable one under semver — and §16.6 makes a build that
> accepts a caller-chosen RNG seed "security-relevant", a decision that
> belongs to the slice with an opinion about the public surface.
> Promotion later is additive; demotion is breaking.

The second blocker is the Instant problem, and it is **survivable but
ugly**: measured above, `Instant::now()` traps on unknown-unknown, but a
minted `Instant` supports every operation the cores need. The seed costs
`unsafe { std::mem::zeroed() }`, sound today only by a private std
layout detail. Acceptable in a demo crate; it does not belong near
slither.

The third is that tokio is unusable on that target for time — confirmed
from tokio's source, `src/time/clock.rs:302`, where even the
**paused** clock seeds its base from `std::time::Instant::now()`. So
shape B really would have to re-implement the driver's timer/output loop
in the demo — i.e. rewrite `src/shell/driver.rs` (1500+ lines) against a
newly-public API, and any divergence makes the demo a misleading picture
of the protocol.

Shape B buys one thing over A: no WASI shim, and a smaller `.wasm`. It
costs a public-API freeze, a soundness hack, and a second driver. **Not
worth it.**

### Shape C — no wasm: recorded run + existing SVGs on Pages

**Verdict: baseline, always available. Effort S — 0.5–1 agent-day.**

Ships today with zero slither changes. What it fails to demonstrate is
exactly what makes slither interesting: the viewer cannot **perturb**
anything. No dragging the loss rate up and watching the retransmit ladder
climb, no forcing a roam and seeing the peer re-home, no stepping the
timers. A recording proves the code runs; it does not let anyone poke the
protocol. Given shape A is measured working, C is the fallback, not the
plan.

## Verdict

**Build shape A.** It is not a plan — it already ran. The full protocol,
real shell and all, executed inside WebAssembly under a WASI preview1
host, handshook, carried stream data, lost packets and retransmitted on
the ratified ladder, and did all of it again in 9.8 ms on a paused clock.
The remaining work is a Web Worker, a shim, and a UI — none of it
protocol work.

### slither-side changes — the complete list

Two hunks. **Both need a maintainer decision** (they are manifest and
`#[cfg]` changes to a released crate), but neither moves a wire byte,
a constant, a timer or a behaviour.

**1. `Cargo.toml` — target-gate `tokio/net`.  [needs maintainer decision]**

```diff
-tokio = { version = "1", features = ["rt", "net", "time", "sync", "macros"] }
+tokio = { version = "1", features = ["rt", "time", "sync", "macros"] }

+[target.'cfg(not(target_family = "wasm"))'.dependencies]
+tokio = { version = "1", features = ["net"] }
```

Cargo unions base and target-specific features, so every non-wasm target
resolves to exactly today's feature set. The manifest comment above the
dep explains `net` and would need its wording updated to say *why* it is
now target-gated.

**2. `src/shell/wire.rs` — cfg the blanket impl.  [needs maintainer decision]**

```diff
+#[cfg(not(target_family = "wasm"))]
 impl Wire for tokio::net::UdpSocket {
```

This is the only production `tokio::net` site in the crate — verified:
every other hit is a doc-comment or inside a `#[cfg(test)]` module
(`src/shell/endpoint.rs:669` is under the `#[cfg(test)]` at line 632).

**Public-API impact:** on wasm targets the `impl Wire for
tokio::net::UdpSocket` disappears. On every native target the public
surface is **unchanged**. Consumers supply their own `Wire` anyway; on
wasm there is no UDP socket to implement it for. Worth an explicit
ruling, since "a trait impl that exists on some targets and not others"
is a real, if narrow, semver-visible fact.

**Not needed:** no `getrandom` change (wasip1 has a backend), no wasm
feature flag, no `WallClock` change, no new dependency, no promotion of
the `core` module, no `SPEC.md` amendment.

**Gate safety, measured** — with both hunks applied:

```
$ cargo check --all-features --all-targets
    Checking slither v0.2.0 (.../agent-a45a2a50f7391112f)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.65s
```

The native gate table is untouched. (The full table was not re-run — this
is a measurement worktree, not a release commit.)

**One caveat to state plainly:** the wasm build works for the **lib
only**. `cargo check --target wasm32-wasip1 --all-targets` still fails on
tokio's wasm gate, because dev-dependencies (`tokio` with `net`, used by
the test suites and `examples/echo.rs`) legitimately need it. That is
correct and not worth fixing — no gate runs against a wasm target.

### Demo-side skeleton

```
demo/                      # NOT a workspace member of the published crate;
  Cargo.toml               #   add to `exclude` in slither's Cargo.toml
  src/main.rs              # wasm32-wasip1 bin: Pair + Network + host calls
  web/
    index.html             # the page
    app.js                 # UI thread: canvas/DOM, receives postMessage
    worker.js              # Worker: browser_wasi_shim + wasm instance
    vendor/browser_wasi_shim/   # vendored, no CDN
    demo.wasm              # build artefact
  Makefile / build.sh      # cargo build --release --target wasm32-wasip1
.github/workflows/pages.yml  # build + upload-pages-artifact + deploy-pages
```

Notes that matter:

- **Web Worker is mandatory**, not a nicety: `block_on` holds its thread
  for the whole run.
- **Use `start_paused(true)`.** It is the fast-forward control *and* it
  keeps the shim off `poll_oneoff`.
- **`@bjorn3/browser_wasi_shim`**, vendored rather than CDN-loaded (GitHub
  Pages, and no external fetch).
- The wasm is ~1–2 MB before `wasm-opt`; fine for Pages.
- Controls worth exposing, each backed by an API that already exists:
  loss/delay/duplication (`FlakyPolicy`), partition and heal
  (`Network::partition`/`heal`), roaming (`Peer::rebind`), send failure
  (`FlakyPolicy::fail_sends`), and a time-advance slider
  (`tokio::time::advance`).
- The packet animation reads `Network::tap()` → `Tap::datagrams()`.

### Effort

| shape | verdict | effort |
|---|---|---|
| **A** wasip1 + shim + Worker | **recommended, proven to the ABI boundary** | **M — 3–5 agent-days** |
| B unknown-unknown, JS-tick over cores | not viable as briefed | L — 8–12 agent-days + a public-API freeze |
| C recorded run on Pages | fallback | S — 0.5–1 agent-day |

Shape A's 3–5 days assumes the two slither hunks are approved. If they
are not, shape A is dead and C is the only option — the cores being
`pub(crate)` closes B independently.

## Conflicts and staleness found (reported, not resolved — rule 3)

**1. The `pub(crate) mod core` rationale is stale on its own terms.**
`src/lib.rs:348-353` justifies keeping the cores private with:

> `pub(crate)` **in this slice**, deliberately: nothing outside the crate
> can drive them **until the driver lands** …

The driver **has** landed — `src/shell/driver.rs` is ~1500 lines and the
shell is complete through round 44. The stated precondition is
discharged, so the comment now argues from a condition that no longer
holds. This does **not** mean the cores should be made public: the
*other* half of the same comment (§16.6's caller-chosen RNG seed being
"security-relevant", and "promotion later is additive; demotion is
breaking") is untouched by the driver landing and may well still be the
maintainer's position. I am reporting the staleness, not resolving it —
this is rule 4(a)'s shape, a sentence whose clauses aged at different
rates, in a comment nothing sweeps.

**2. Not a conflict, but worth recording:** `Cargo.toml`'s comment on the
`test-util` feature claims *"Dependency-free — `FlakyWire` needs only
`tokio/time` and `tokio/sync`, both hard deps."* Measured **correct** —
`testutil` additionally uses `tokio/rt` (`LocalSet`, `spawn_local`) and
`tokio/macros` (`select!`, `pin!`), but those are also hard deps, so the
claim's substance holds. Noted only because I checked it rather than
assuming it.

## Worktree state

This worktree has the throwaway probe applied and is **NOT to be merged**:

- `Cargo.toml`: `tokio` `net` moved to a `[target.'cfg(not(target_family
  = "wasm"))'.dependencies]` block; `getrandom` given `features =
  ["wasm_js"]` (the second is unknown-unknown-only and is **not** part of
  the recommendation).
- `src/shell/wire.rs`: `#[cfg(not(target_family = "wasm"))]` on the
  `UdpSocket` impl.

Probe crates live in the scratchpad, outside the repo:
`scratchpad/instprobe` (the Instant question) and `scratchpad/wasidemo`
(the shape-A proof).

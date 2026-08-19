# The slither browser demo

Two slither endpoints, compiled to WebAssembly, handshaking and talking to
each other inside a browser tab. Ruling 277 is what makes it possible: the
library — `test-util`'s in-memory `FlakyWire` fabric included — compiles
for `wasm32-wasip1`.

**Nothing here is part of the published crate.** `/demo` is in slither's
`exclude`, the runner is not a workspace member, and no gate in the release
table runs against it.

## What is real, and what is not

Real: the IK handshake, the packet layer, the replay window, the
retransmission ladder, the frame layer, the timers, both state machines,
the shell driver. All of it is the crate, unmodified.

Not real: the network. A browser tab has no UDP socket. The two endpoints
exchange datagrams through `slither::testutil::Network`, the same
kernel-free fabric §16.10 requires and every slither flow test rides — so
this is less a workaround than a demonstration of the testability claim.
The loss, duplication and delay controls are a simulator behind the
crate's public `Wire` trait (`demo/runner/src/wire.rs`, and read its
module doc before changing anything about how fates are decided).

Timers run on tokio's **paused clock**, so protocol time is virtual: a
25-second liveness timeout resolves in about five milliseconds of wall
time. Every `t_us` in the event stream is virtual microseconds. The page
says so; keep it saying so.

## Layout

```
demo/
  runner/                     a wasm32-wasip1 command binary, its own crate
    src/main.rs               argv in, JSON lines out, one scenario per run
    src/sim.rs                two endpoints; §6.2's ladder, climbed audibly
    src/scenarios.rs          the four scenarios
    src/wire.rs               ObservedWire — the fault injector that reports
    src/trace.rs              §18.2's five trace targets → timeline events
    src/log.rs                the event log and its JSON
  web/                        framework-free static page
    index.html  style.css  app.js
    worker.js                 the Web Worker; block_on must not run on the UI thread
    wasm-run.js               the one place that starts the wasm
    vendor/browser_wasi_shim/ vendored, pinned, with licences — see PROVENANCE.md
  verify/                     what can be checked without a browser
  build.sh                    assembles _site/
```

## Build and run locally

```sh
rustup target add wasm32-wasip1
./demo/build.sh
python3 -m http.server -d demo/_site 8000   # then open http://localhost:8000
```

A plain `file://` open will not work: ES modules and `WebAssembly` both
need an HTTP origin.

## Verify

```sh
cd demo/runner && cargo test          # the runner's own units
node demo/verify/check-web.mjs        # modules parse; nothing reaches a CDN
node demo/verify/run-vendored-shim.mjs # every scenario through the vendored shim
node demo/verify/render-smoke.mjs     # app.js driven for real against a stub DOM
node demo/verify/run-node-wasi.mjs -- --scenario lossy --loss 0.35 --seed 6
```

`.github/workflows/pages.yml` runs the first four before it deploys.

## The runner's interface

```
slither-demo [--scenario clean|lossy|duplicate|partition]
             [--seed N] [--loss 0..1] [--duplicate 0..1]
             [--delay-ms N] [--jitter-ms N] [--message TEXT]
```

One JSON object per line on stdout, each with a monotonic `seq` and a
virtual `t_us`; the last is always `end`. The schema is tabulated in
`.spec-v2-clean-slate/release-prep-demo-build.md`.

Everything is a function of `--seed`: the network's fault draws, both
endpoints' RNGs and both static keys. The same URL parameters produce the
same run, on any machine.

## If you change the runner

The page reads specific field names off specific event kinds
(`app.js`'s `describe` and `syncPanels`). `run-vendored-shim.mjs` asserts
the ones the animation cannot do without, but it cannot know about a field
you add and forget to render. Run `render-smoke.mjs` — it executes the real
page logic over a real run and will tell you if a counter stops moving.

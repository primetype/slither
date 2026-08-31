// Run every scenario through the **vendored** browser_wasi_shim, using
// the same `wasm-run.js` the page's Worker uses.
//
//   node demo/verify/run-vendored-shim.mjs
//
// What this proves, and what it does not:
//
//   PROVES  the vendored shim executes the .wasm, that `wasm-run.js`'s
//           argv construction and stdout wiring are right, that every
//           scenario reaches `end` with `ok:true`, that every emitted line
//           is valid JSON, and that `poll_oneoff` is never called — the
//           one host call that would freeze a Worker, because this shim
//           implements it as a busy-wait spin.
//
//   DOES NOT PROVE anything about a real browser: no Worker, no module
//           loader, no `fetch`, no rendering. Node's ESM loader and a
//           browser's are close but not identical, and only a click-through
//           settles it.

import { readFile } from "node:fs/promises";
import { argv, exit } from "node:process";
import { runScenario, argvFor } from "../web/wasm-run.js";

const wasmPath =
  argv[2] ??
  new URL(
    "../runner/target/wasm32-wasip1/release/slither-demo.wasm",
    import.meta.url,
  );

const module = await WebAssembly.compile(await readFile(wasmPath));

// The scenarios the MVP claims, each with parameters chosen to actually
// exercise the thing it is named for. `lossy` uses seed 6 because that is
// a seed on which msg1 itself dies twice — a lossy run in which nothing is
// lost would pass this file while proving nothing (a bound the degenerate
// case must violate).
const CASES = [
  { name: "clean", opts: { scenario: "clean", seed: 0xc0ffee } },
  {
    name: "lossy",
    opts: { scenario: "lossy", seed: 6, loss: 0.35 },
    expect: (ev) => ({
      "msg1 was dropped at least once": ev.some(
        (e) => e.kind === "tx" && e.type === "init" && e.fate === "lost",
      ),
      "the retransmit ladder advanced virtual time past 5 s": ev.some(
        (e) => e.t_us >= 5_000_000,
      ),
    }),
  },
  {
    name: "duplicate",
    opts: { scenario: "duplicate", seed: 3, duplicate: 0.6 },
    expect: (ev) => ({
      "a duplicate copy was delivered": ev.some(
        (e) => e.kind === "tx" && e.copy === 1,
      ),
      "the replay window rejected one": ev.some(
        (e) => e.kind === "trace" && e.target === "slither::replay",
      ),
    }),
  },
  {
    name: "partition",
    opts: { scenario: "partition", seed: 5 },
    expect: (ev) => ({
      "datagrams were blackholed": ev.some(
        (e) => e.kind === "tx" && e.fate === "blackholed",
      ),
      "both sides died on the liveness bound": (() => {
        const lost = ev.filter((e) => e.kind === "lost");
        return (
          lost.length === 2 && lost.every((e) => /dead timeout/.test(e.cause))
        );
      })(),
    }),
  },
];

// The fields `demo/web/app.js` reads off each event kind, in `describe`
// and `syncPanels`. Mirrored here by hand rather than inferred, so that a
// runner change which drops one fails a check instead of quietly rendering
// `undefined` into the timeline. Kept minimal on purpose: only what the
// page cannot draw without.
const REQUIRED = {
  run: ["scenario", "seed", "loss", "duplicate"],
  topology: ["a", "b"],
  stage: ["side", "stage", "dhs"],
  tx: ["pkt", "copy", "from", "to", "len", "type", "fate"],
  trace: ["target", "level", "message", "fields"],
  app: ["side", "op"],
  lost: ["side", "cause"],
  fault: ["what", "detail"],
  note: ["text"],
  end: ["ok", "wall_us", "sent", "lost", "duplicated", "blackholed"],
};

let failures = 0;
const fail = (msg) => {
  console.log(`  FAIL  ${msg}`);
  failures += 1;
};
const pass = (msg) => console.log(`  ok    ${msg}`);

for (const { name, opts, expect } of CASES) {
  const args = argvFor(opts);
  console.log(`\n${name}: slither-demo ${args.join(" ")}`);

  const lines = [];
  const stderr = [];
  const result = runScenario(module, args, (l) => lines.push(l), {
    onStderr: (l) => stderr.push(l),
  });

  // 1. Every line parses.
  const events = [];
  for (const [i, line] of lines.entries()) {
    try {
      events.push(JSON.parse(line));
    } catch (e) {
      fail(`line ${i} is not JSON: ${line.slice(0, 120)}`);
    }
  }
  if (events.length === lines.length) pass(`${lines.length} lines, all JSON`);

  // 2. It finished, cleanly.
  if (result.exitCode !== 0) fail(`exit code ${result.exitCode}`);
  else pass("exit code 0");
  if (stderr.length) fail(`stderr: ${stderr.join(" | ")}`);

  const end = events.at(-1);
  if (!end || end.kind !== "end") fail("the last event is not `end`");
  else if (end.ok !== true) fail(`end.ok is false: ${end.error}`);
  else
    pass(
      `end ok — ${end.sent} sent, ${end.delivered} delivered, ` +
        `${end.lost} lost, ${end.duplicated} duplicated, ` +
        `${end.blackholed} blackholed`,
    );

  // 3. `seq` is dense and monotonic — the UI sorts on it.
  const seqOk = events.every((e, i) => e.seq === i);
  seqOk ? pass("seq is dense from 0") : fail("seq is not dense from 0");

  // 4. Virtual time never goes backwards.
  const timeOk = events.every((e, i) => i === 0 || e.t_us >= events[i - 1].t_us);
  timeOk ? pass("t_us is monotonic") : fail("t_us went backwards");

  // 5. The host call that would freeze a Worker.
  if (result.pollOneoffCalls !== 0)
    fail(
      `poll_oneoff was called ${result.pollOneoffCalls}× — the runtime ` +
        `parked on a real timer, and this shim spins while waiting`,
    );
  else pass("poll_oneoff: 0 calls");

  // 6. Virtual time is not wall time, and the ratio is the whole pitch.
  const virtualMs = Math.round(end?.t_us / 1000) || 0;
  pass(`${virtualMs} ms of protocol time in ${result.wallMs} ms of wall time`);

  // 7. Every event carries what the page reads off it.
  const shapeGaps = new Set();
  for (const e of events) {
    for (const f of REQUIRED[e.kind] ?? []) {
      if (e[f] === undefined) shapeGaps.add(`${e.kind}.${f}`);
    }
    if (!(e.kind in REQUIRED) && e.kind !== "error" && e.kind !== "stderr") {
      shapeGaps.add(`unknown kind "${e.kind}" — app.js will not render it`);
    }
  }
  if (shapeGaps.size)
    fail(`fields the page needs are missing: ${[...shapeGaps].join(", ")}`);
  else
    pass(
      `every event carries the fields app.js reads ` +
        `(${new Set(events.map((e) => e.kind)).size} kinds seen)`,
    );

  // 8. Scenario-specific claims.
  for (const [what, ok] of Object.entries(expect?.(events) ?? {})) {
    ok ? pass(what) : fail(what);
  }
}

console.log(
  failures === 0
    ? "\nALL CHECKS PASSED"
    : `\n${failures} CHECK(S) FAILED`,
);
exit(failures === 0 ? 0 : 1);

// Running the slither demo runner through the vendored WASI shim.
//
// This module is imported by two callers and is deliberately the *only*
// place that knows how the wasm is started:
//
//   * `worker.js`, inside the page's Web Worker;
//   * `demo/verify/run-vendored-shim.mjs`, under Node.
//
// One implementation means the thing verified on the command line is the
// thing the browser runs. Two would only prove that two files exist.

import {
  WASI,
  ConsoleStdout,
  OpenFile,
  File,
} from "./vendor/browser_wasi_shim/index.js";

/**
 * Run one scenario to completion.
 *
 * `block_on` holds its thread for the whole run, so this must not be
 * called on a browser's main thread — that is what the Worker is for.
 *
 * @param {WebAssembly.Module} module compiled `slither-demo.wasm`
 * @param {string[]} args argv after the program name
 * @param {(line: string) => void} onLine one JSON line of the timeline
 * @param {{onStderr?: (line: string) => void}} [opts]
 * @returns {{exitCode: number, pollOneoffCalls: number, wallMs: number}}
 */
export function runScenario(module, args, onLine, opts = {}) {
  const onStderr = opts.onStderr ?? (() => {});
  const fds = [
    // stdin: empty. The runner never reads it — steering would need
    // `Atomics.wait` on a `SharedArrayBuffer`, and GitHub Pages sends no
    // COOP/COEP headers, so that is not available here at all.
    new OpenFile(new File([])),
    ConsoleStdout.lineBuffered(onLine),
    ConsoleStdout.lineBuffered(onStderr),
  ];

  // `{ debug: false }` is NOT the default, despite appearances. The shim
  // does `debug.enable(options.debug)` and `enable(undefined)` resolves to
  // **true** (`enabled === undefined ? true : enabled`), so omitting the
  // options object turns the syscall logger ON and prints a line per
  // `environ_sizes_get`/`clock_res_get` to the console. Measured: the
  // verifier printed `wasi: 5 46` before this argument was added.
  const wasi = new WASI(["slither-demo", ...args], [], fds, { debug: false });

  // Count host calls the demo claims never to make. `poll_oneoff` is a
  // busy-wait spin in this shim (see vendor/PROVENANCE.md), so one call
  // for a 25-second protocol timeout would freeze the Worker for 25
  // seconds. The paused clock is what prevents it; this counter is what
  // proves the prevention still works.
  let pollOneoffCalls = 0;
  const imports = { ...wasi.wasiImport };
  const realPoll = imports.poll_oneoff;
  imports.poll_oneoff = (...a) => {
    pollOneoffCalls += 1;
    return realPoll(...a);
  };

  const instance = new WebAssembly.Instance(module, {
    wasi_snapshot_preview1: imports,
  });

  const started = Date.now();
  // `start` returns the exit code, translating the shim's `WASIProcExit`.
  // Anything else — a Rust panic lowers to an `unreachable` trap, because
  // the release profile is `panic = "abort"` — propagates to the caller,
  // which is right: a trapped run must not look like a finished one.
  const exitCode = wasi.start(instance);
  return { exitCode, pollOneoffCalls, wallMs: Date.now() - started };
}

/**
 * Turn the UI's control values into the runner's argv.
 *
 * Kept here so the Worker and the verifier cannot disagree about flag
 * names, and so a value the runner would reject never leaves the page.
 */
export function argvFor({
  scenario,
  seed,
  loss,
  duplicate,
  delayMs,
  jitterMs,
  message,
}) {
  const args = ["--scenario", String(scenario), "--seed", String(seed >>> 0)];
  const push = (flag, value) => {
    if (value !== undefined && value !== null) args.push(flag, String(value));
  };
  push("--loss", loss);
  push("--duplicate", duplicate);
  push("--delay-ms", delayMs);
  push("--jitter-ms", jitterMs);
  if (message) args.push("--message", message);
  return args;
}

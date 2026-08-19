// The Web Worker that actually runs slither.
//
// It exists for one reason: the runner's `block_on` holds its thread for
// the whole scenario. On the main thread that is a frozen tab; here it is
// nobody's problem, and `postMessage` queues rather than blocking, so the
// page stays responsive throughout.
//
// Protocol with the page:
//   → { type: "wasm", bytes: ArrayBuffer }  compile once, reuse forever
//   → { type: "run", id, args: string[] }   run one scenario
//   ← { type: "ready" }
//   ← { type: "done", id, lines, exitCode, wallMs, pollOneoffCalls }
//   ← { type: "failed", id, error }
//
// Lines are batched into one message rather than streamed: a whole run is
// a few hundred lines and finishes in milliseconds, so streaming would buy
// nothing and cost a message per event.

import { runScenario } from "./wasm-run.js";

let module = null;

self.onmessage = async (ev) => {
  const msg = ev.data;

  if (msg.type === "wasm") {
    try {
      module = await WebAssembly.compile(msg.bytes);
      self.postMessage({ type: "ready" });
    } catch (e) {
      self.postMessage({ type: "failed", id: null, error: String(e) });
    }
    return;
  }

  if (msg.type === "run") {
    if (!module) {
      self.postMessage({
        type: "failed",
        id: msg.id,
        error: "the wasm module has not been compiled yet",
      });
      return;
    }
    const lines = [];
    try {
      const { exitCode, wallMs, pollOneoffCalls } = runScenario(
        module,
        msg.args,
        (line) => lines.push(line),
        { onStderr: (line) => lines.push(stderrEvent(line)) },
      );
      self.postMessage({
        type: "done",
        id: msg.id,
        lines,
        exitCode,
        wallMs,
        pollOneoffCalls,
      });
    } catch (e) {
      // A Rust panic is `panic = "abort"`, which lowers to an
      // `unreachable` trap and arrives here as a RuntimeError. Whatever
      // was emitted before it is still worth showing, so it travels with
      // the failure rather than being discarded.
      self.postMessage({
        type: "failed",
        id: msg.id,
        error: String(e && e.message ? e.message : e),
        lines,
      });
    }
  }
};

/** Wrap a stderr line as an event so it lands on the same timeline. */
function stderrEvent(line) {
  return JSON.stringify({
    seq: -1,
    t_us: 0,
    kind: "stderr",
    text: line,
  });
}

// Run the demo runner under Node's own WASI preview1 host.
//
// This is the coarse check: does the .wasm execute at all, does every
// scenario reach its `end` event, and does every line parse as JSON. The
// finer check — that the *vendored* shim the browser will use agrees with
// the same binary — is `run-vendored-shim.mjs`.
//
//   node demo/verify/run-node-wasi.mjs [--wasm path] [-- args...]
//
// Node 22 keeps `node:wasi` behind an experimental warning; it is the same
// preview1 ABI `@bjorn3/browser_wasi_shim` implements.

import { WASI } from "node:wasi";
import { readFile } from "node:fs/promises";
import { argv, exit } from "node:process";

const WASM_DEFAULT = new URL(
  "../runner/target/wasm32-wasip1/release/slither-demo.wasm",
  import.meta.url,
);

const split = argv.indexOf("--");
const own = split === -1 ? argv.slice(2) : argv.slice(2, split);
const scenarioArgs = split === -1 ? [] : argv.slice(split + 1);
const wasmIdx = own.indexOf("--wasm");
const wasmPath = wasmIdx === -1 ? WASM_DEFAULT : own[wasmIdx + 1];

const bytes = await readFile(wasmPath);
const module = await WebAssembly.compile(bytes);

const wasi = new WASI({
  version: "preview1",
  args: ["slither-demo", ...scenarioArgs],
  env: {},
});

const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
let code = 0;
try {
  code = wasi.start(instance) ?? 0;
} catch (e) {
  console.error("TRAPPED:", e.message);
  exit(70);
}
exit(code);

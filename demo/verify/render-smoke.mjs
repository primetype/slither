// Run `app.js` for real, against a stub DOM, on a real wasm run.
//
//   node demo/verify/render-smoke.mjs
//
// `check-web.mjs` proves the modules parse. That is a low bar: a typo in a
// field name, a method that does not exist on an element, a canvas call
// with the wrong arity — none of those are syntax errors, and all of them
// are blank screens. This file boots the actual page module and drives
// three hundred animation frames over a genuine timeline, so the page's
// logic executes end to end.
//
// The Worker stub is the interesting part: instead of faking a response it
// runs `runScenario` **in process, through the vendored shim**, and speaks
// exactly the message protocol `worker.js` speaks. So this also pins that
// `app.js` and `worker.js` agree about the shape of `done` and `failed` —
// which is a contract between two files nothing else checks.
//
// What it does NOT prove: that anything is legible. No pixels are
// compared, no layout is measured. Only a browser settles that.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { exit } from "node:process";
import { runScenario } from "../web/wasm-run.js";

const wasmPath = fileURLToPath(
  new URL("../runner/target/wasm32-wasip1/release/slither-demo.wasm", import.meta.url),
);
const wasmModule = await WebAssembly.compile(await readFile(wasmPath));

let failures = 0;
const fail = (m) => {
  console.log(`  FAIL  ${m}`);
  failures += 1;
};
const pass = (m) => console.log(`  ok    ${m}`);

// ── a DOM, to the extent app.js uses one ───────────────────────────────

let canvasOps = 0;

class El {
  constructor(tag = "div", id = "") {
    this.tagName = tag;
    this.id = id;
    this.children = [];
    this.dataset = {};
    this.style = {};
    this._text = "";
    this.value = "0";
    this.attributes = {};
    this.listeners = {};
    this.clientWidth = 960;
    this.width = 960;
    this.height = 520;
    this.classList = {
      _set: new Set(),
      add: (c) => this.classList._set.add(c),
      remove: (c) => this.classList._set.delete(c),
      toggle: (c, on) =>
        on ? this.classList._set.add(c) : this.classList._set.delete(c),
      contains: (c) => this.classList._set.has(c),
    };
  }
  get textContent() {
    return this._text;
  }
  set textContent(v) {
    this._text = String(v);
    if (v === "") this.children = [];
  }
  get childElementCount() {
    return this.children.length;
  }
  appendChild(c) {
    this.children.push(c);
    return c;
  }
  setAttribute(k, v) {
    this.attributes[k] = v;
  }
  getAttribute(k) {
    return this.attributes[k];
  }
  addEventListener(t, f) {
    (this.listeners[t] ||= []).push(f);
  }
  dispatch(t, ev = {}) {
    for (const f of this.listeners[t] || []) f({ target: this, ...ev });
  }
  scrollIntoView() {}
  querySelectorAll(sel) {
    return document.querySelectorAll(sel);
  }
  getContext() {
    // Every 2D method app.js calls, counting the calls so "it drew
    // nothing" is distinguishable from "it drew".
    const noop = () => {
      canvasOps += 1;
    };
    return new Proxy(
      {
        measureText: (t) => ({ width: String(t).length * 6 }),
        canvas: this,
      },
      {
        get(target, prop) {
          if (prop in target) return target[prop];
          return noop;
        },
        set() {
          return true;
        },
      },
    );
  }
}

const registry = new Map();
const el = (id, tag = "div") => {
  if (!registry.has(id)) registry.set(id, new El(tag, id));
  return registry.get(id);
};

// The page's controls, with the defaults index.html ships.
for (const [id, v] of [
  ["loss", "0"],
  ["duplicate", "0"],
  ["delay", "0"],
  ["jitter", "0"],
  ["seed", "6"],
  ["message", "hello from WebAssembly"],
  ["speed", "1"],
  ["scrub", "1000"],
]) {
  el(id).value = v;
}

const scenarioButtons = ["clean", "lossy", "duplicate", "partition"].map((s) => {
  const b = new El("button", "btn-" + s);
  b.dataset.scenario = s;
  return b;
});
const badges = [
  ...["dial", "established"].map((s) => tagBadge(s, "a")),
  ...["intro", "claimed", "proven", "connection"].map((s) => tagBadge(s, "b")),
];
function tagBadge(stage, side) {
  const b = new El("span", "badge-" + side + "-" + stage);
  b.dataset.stage = stage;
  b._side = side;
  return b;
}

const document = {
  documentElement: new El("html"),
  getElementById: (id) => el(id, id === "canvas" ? "canvas" : "div"),
  createElement: (tag) => new El(tag),
  createTextNode: (text) => {
    const n = new El("#text");
    n.textContent = String(text);
    return n;
  },
  querySelectorAll: (sel) => {
    if (sel === "#scenarios button") return scenarioButtons;
    if (sel === ".badge") return badges;
    if (sel === "#ladder-a .badge") return badges.filter((b) => b._side === "a");
    if (sel === "#ladder-b .badge") return badges.filter((b) => b._side === "b");
    return [];
  },
};

// ── the Worker stub, speaking worker.js's protocol ─────────────────────

class FakeWorker {
  constructor() {
    this.onmessage = null;
    this.onerror = null;
    this.module = null;
  }
  postMessage(msg) {
    queueMicrotask(() => {
      if (msg.type === "wasm") {
        this.module = wasmModule;
        this.onmessage?.({ data: { type: "ready" } });
        return;
      }
      if (msg.type === "run") {
        const lines = [];
        try {
          const r = runScenario(this.module, msg.args, (l) => lines.push(l));
          this.onmessage?.({
            data: {
              type: "done",
              id: msg.id,
              lines,
              exitCode: r.exitCode,
              wallMs: r.wallMs,
              pollOneoffCalls: r.pollOneoffCalls,
            },
          });
        } catch (e) {
          this.onmessage?.({
            data: { type: "failed", id: msg.id, error: String(e), lines },
          });
        }
      }
    });
  }
}

// ── globals ────────────────────────────────────────────────────────────

let clock = 0;
const frameQueue = [];

Object.assign(globalThis, {
  document,
  window: {
    devicePixelRatio: 2,
    matchMedia: () => ({ addEventListener() {} }),
  },
  Worker: FakeWorker,
  performance: { now: () => clock },
  requestAnimationFrame: (fn) => frameQueue.push(fn),
  getComputedStyle: () => ({ getPropertyValue: () => "" }),
  fetch: async () => ({
    ok: true,
    status: 200,
    arrayBuffer: async () => (await readFile(wasmPath)).buffer,
  }),
});
globalThis.window.matchMedia = globalThis.window.matchMedia;

// `app.js` throws inside a rAF callback rather than at top level, so an
// unhandled rejection or exception must fail the run rather than warn.
process.on("unhandledRejection", (e) => {
  fail("unhandled rejection: " + e);
});

// ── drive it ───────────────────────────────────────────────────────────

console.log("\nbooting app.js against a stub DOM");
await import("../web/app.js");

// Let boot()'s fetch + compile resolve.
for (let i = 0; i < 50; i++) await new Promise((r) => setTimeout(r, 0));

let frames = 0;
let thrown = null;
for (let i = 0; i < 300 && !thrown; i++) {
  const fn = frameQueue.shift();
  if (!fn) {
    await new Promise((r) => setTimeout(r, 1));
    continue;
  }
  clock += 33;
  try {
    fn(clock);
    frames += 1;
  } catch (e) {
    thrown = e;
  }
  await Promise.resolve();
}

if (thrown) fail(`a frame threw: ${thrown.stack?.split("\n").slice(0, 3).join(" | ")}`);
else pass(`${frames} animation frames, no exception`);

if (canvasOps > 500) pass(`${canvasOps} canvas operations issued`);
else fail(`only ${canvasOps} canvas operations — the diagram drew nothing`);

if (el("log").childElementCount > 10)
  pass(`${el("log").childElementCount} log rows rendered`);
else fail(`log has ${el("log").childElementCount} rows`);

const datagrams = Number(el("m-datagrams").textContent);
if (datagrams > 0) pass(`counter "datagrams sent" reached ${datagrams}`);
else fail(`counter "datagrams sent" is ${el("m-datagrams").textContent}`);

const dh = el("m-dh").textContent;
if (/^\d+ \/ \d+$/.test(dh) && dh !== "0 / 0") pass(`DH counter reads ${dh}`);
else fail(`DH counter reads "${dh}"`);

const lit = badges.filter((b) => b.classList.contains("on")).map((b) => b.dataset.stage);
if (lit.length >= 4) pass(`staged-accept badges lit: ${lit.join(", ")}`);
else fail(`only ${lit.length} badges lit: ${lit.join(", ")}`);

if (el("status").textContent.includes("never waited on a real clock"))
  pass(`status line: "${el("status").textContent}"`);
else fail(`status line is "${el("status").textContent}"`);

// Switching scenarios must re-run, not wedge. Click "lossy" and pump on.
console.log("\nswitching scenario to lossy");
scenarioButtons[1].dispatch("click");
for (let i = 0; i < 40; i++) await new Promise((r) => setTimeout(r, 0));
let more = 0;
for (let i = 0; i < 200; i++) {
  const fn = frameQueue.shift();
  if (!fn) {
    await new Promise((r) => setTimeout(r, 1));
    continue;
  }
  clock += 33;
  try {
    fn(clock);
    more += 1;
  } catch (e) {
    fail("a frame threw after switching scenario: " + e.message);
    break;
  }
  await Promise.resolve();
}
// Not `pass(more)`: a wedged page yields zero frames, and a check that
// reports whatever number it finds passes on exactly the failure it exists
// to catch.
if (more > 50) pass(`${more} further frames after the scenario switch`);
else fail(`only ${more} frames after the scenario switch — the page wedged`);
const retx = Number(el("m-retx").textContent);
if (retx > 0) pass(`handshake retries counter reached ${retx} on the lossy run`);
else fail(`handshake retries counter is ${el("m-retx").textContent} on a lossy run`);

console.log(failures === 0 ? "\nALL CHECKS PASSED" : `\n${failures} CHECK(S) FAILED`);
exit(failures === 0 ? 0 : 1);

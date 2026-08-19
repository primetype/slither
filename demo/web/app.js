// The page: controls in, one Worker run, one animated timeline out.
//
// The diagram is a sequence diagram that plays. Vertical is time, the two
// vertical rules are the endpoints, and each datagram is an arrow crossing
// between them as the playhead reaches its row. A packet the network
// destroyed stops halfway and is struck out; one a blackhole swallowed
// barely leaves the lifeline.
//
// # Why vertical spacing is not proportional to time
//
// Because a run's virtual time is either degenerate or enormous, often in
// the same page. A clean handshake happens entirely at t = 0 — correctly:
// virtual time only advances when something waits, and nothing does — so a
// strictly proportional axis would stack every event on one pixel. A lossy
// handshake spans fourteen seconds, so the same axis would put two events
// fourteen thousand pixels apart. Rows therefore get a fixed minimum
// height plus a `sqrt`-compressed gap, and the **gutter prints the real
// virtual timestamp** on every row where it changed. Ordering and the
// timestamps are exact; only the spacing is squashed, and the spacing is
// the part nobody should be measuring off a screenshot.

import { argvFor } from "./wasm-run.js";

const WASM_URL = "./slither-demo.wasm";

const BLURBS = {
  clean:
    "Dial, handshake, one message each way on a bidirectional stream, " +
    "clean close. Watch §6.2's staged accept: intro → claimed → proven → " +
    "connection, with the Diffie–Hellman cost charged one rung at a time.",
  lossy:
    "The same application over a wire that drops packets. When msg1 dies, " +
    "the initiator waits out REKEY_TIMEOUT and retransmits with a fresh " +
    "ephemeral — so each retry costs two more DH operations, and the gaps " +
    "on the axis are five seconds of protocol time each.",
  duplicate:
    "Every datagram may arrive twice. The session does not care: the " +
    "second copy fails the replay window and is discarded, which slither " +
    "reports on its slither::replay trace target. No session state moves.",
  partition:
    "Established, exchanging, and then the path disappears. Nothing is " +
    "signalled, because there is nobody to signal. A keeps sending; every " +
    "send succeeds and reaches no one; and the connection dies in silence " +
    "on the liveness bound, twenty-five seconds later.",
};

// ── layout constants ───────────────────────────────────────────────────
const ROW = 26; // minimum vertical space per event
const GAP_K = 4; // px per sqrt(ms) of virtual gap
const GAP_MAX = 420;
const TOP_PAD = 26;
const BOTTOM_PAD = 60;
const GUTTER = 84;
const FLIGHT = 30; // px of playhead travel = one packet's flight
const PX_PER_SEC = 190;

// ── DOM ────────────────────────────────────────────────────────────────
const $ = (id) => document.getElementById(id);
const canvas = $("canvas");
const ctx = canvas.getContext("2d");
const statusEl = $("status");

const controls = {
  loss: $("loss"),
  duplicate: $("duplicate"),
  delay: $("delay"),
  jitter: $("jitter"),
  seed: $("seed"),
  message: $("message"),
};

// ── state ──────────────────────────────────────────────────────────────
let worker = null;
let scenario = "clean";
let events = [];
let rows = [];
let contentHeight = 0;
let head = 0;
let playing = false;
let lastFrame = 0;
let lastIndex = -1;
let runId = 0;
let pending = null;
let summary = { wallMs: 0 };
let theme = readTheme();

// ── boot ───────────────────────────────────────────────────────────────

boot();

async function boot() {
  wireControls();
  try {
    worker = new Worker(new URL("./worker.js", import.meta.url), {
      type: "module",
    });
  } catch (e) {
    return fatal(
      "This browser refused to start a module Web Worker, which is where " +
        "slither runs. (" + e + ")",
    );
  }
  worker.onmessage = onWorkerMessage;
  worker.onerror = (e) => fatal("Worker error: " + (e.message || e));

  let bytes;
  try {
    const res = await fetch(WASM_URL);
    if (!res.ok) throw new Error(res.status + " " + res.statusText);
    bytes = await res.arrayBuffer();
  } catch (e) {
    return fatal(
      "Could not load " + WASM_URL + " (" + e.message + "). If you are " +
        "opening this file directly, serve the directory over HTTP instead — " +
        "modules and WebAssembly both need it.",
    );
  }
  worker.postMessage({ type: "wasm", bytes }, [bytes]);
  setStatus("Compiling " + (bytes.byteLength / 1024).toFixed(0) + " KB of WebAssembly…");
  requestAnimationFrame(frame);
}

function onWorkerMessage(ev) {
  const msg = ev.data;
  if (msg.type === "ready") {
    setStatus("");
    run();
    return;
  }
  if (msg.type === "failed") {
    if (msg.id !== null && msg.id !== runId) return;
    load(msg.lines || [], { wallMs: 0 });
    fatal("The run trapped: " + msg.error);
    return;
  }
  if (msg.type === "done") {
    if (msg.id !== runId) return; // a stale run; a newer one is in flight
    load(msg.lines, msg);
    setStatus(
      msg.lines.length +
        " events · " +
        msg.wallMs +
        " ms of your CPU · " +
        (msg.pollOneoffCalls === 0
          ? "the runtime never waited on a real clock"
          : msg.pollOneoffCalls + " host waits (unexpected)"),
    );
  }
}

// ── running ────────────────────────────────────────────────────────────

function run() {
  if (!worker) return;
  const id = ++runId;
  const args = argvFor({
    scenario,
    seed: Number(controls.seed.value) || 0,
    loss: Number(controls.loss.value) / 100,
    duplicate: Number(controls.duplicate.value) / 100,
    delayMs: Number(controls.delay.value),
    jitterMs: Number(controls.jitter.value),
    message: controls.message.value,
  });
  worker.postMessage({ type: "run", id, args });
}

/** Debounced, because a slider drags through fifty values. */
function schedule() {
  clearTimeout(pending);
  pending = setTimeout(run, 90);
}

function load(lines, meta) {
  events = [];
  for (const line of lines) {
    try {
      events.push(JSON.parse(line));
    } catch {
      /* a truncated final line from a trapped run; drop it */
    }
  }
  summary = meta;
  layout();
  head = 0;
  // -2, not -1: `indexAt(0)` is -1 (nothing has been reached yet), so
  // `lastIndex = -1` makes the first `syncPanels` a no-op and leaves the
  // *previous* run's counters and badges on screen until the playhead
  // passes the first row. -2 forces one reset pass immediately.
  lastIndex = -2;
  playing = true;
  lastFrame = performance.now();
  $("play").textContent = "Pause";
}

function layout() {
  rows = [];
  let y = TOP_PAD;
  let prev = events.length ? events[0].t_us : 0;
  for (const e of events) {
    const gapMs = Math.max(0, (e.t_us - prev) / 1000);
    y += gapMs > 0 ? Math.min(GAP_MAX, GAP_K * Math.sqrt(gapMs)) : 0;
    y += ROW;
    rows.push(y);
    prev = e.t_us;
  }
  contentHeight = y + BOTTOM_PAD;
}

// ── the frame loop ─────────────────────────────────────────────────────

function frame(now) {
  const dt = Math.min(0.1, (now - lastFrame) / 1000);
  lastFrame = now;
  if (playing) {
    head += dt * PX_PER_SEC * Number($("speed").value);
    if (head >= contentHeight) {
      head = contentHeight;
      playing = false;
      $("play").textContent = "Replay";
    }
    $("scrub").value = String(
      contentHeight > 0 ? Math.round((head / contentHeight) * 1000) : 1000,
    );
  }
  draw();
  syncPanels();
  requestAnimationFrame(frame);
}

// ── drawing ────────────────────────────────────────────────────────────

function draw() {
  const dpr = window.devicePixelRatio || 1;
  const cssW = canvas.clientWidth;
  if (canvas.width !== Math.round(cssW * dpr)) {
    canvas.width = Math.round(cssW * dpr);
    canvas.height = Math.round(520 * dpr);
    canvas.style.height = "520px";
  }
  const H = 520;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, H);

  const xa = GUTTER + (cssW - GUTTER) * 0.2;
  const xb = GUTTER + (cssW - GUTTER) * 0.8;
  const scroll = clamp(head - H * 0.55, 0, Math.max(0, contentHeight - H));
  const Y = (y) => y - scroll;

  // lifelines
  ctx.strokeStyle = theme.line;
  ctx.lineWidth = 1;
  for (const x of [xa, xb]) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, H);
    ctx.stroke();
  }

  ctx.font = "11px " + theme.mono;
  ctx.textBaseline = "middle";

  let lastLabel = null;
  for (let i = 0; i < events.length; i++) {
    const y = Y(rows[i]);
    if (y < -40 || y > H + 40) continue;
    const e = events[i];
    const revealed = head >= rows[i];
    if (!revealed) continue;

    // gutter timestamp, once per distinct virtual instant
    const label = formatTime(e.t_us);
    if (label !== lastLabel) {
      ctx.fillStyle = theme.note;
      ctx.textAlign = "right";
      ctx.fillText(label, GUTTER - 12, y);
      ctx.strokeStyle = theme.line;
      ctx.beginPath();
      ctx.moveTo(GUTTER - 4, y);
      ctx.lineTo(cssW, y);
      ctx.globalAlpha = 0.35;
      ctx.stroke();
      ctx.globalAlpha = 1;
      lastLabel = label;
    }

    switch (e.kind) {
      case "tx":
        drawPacket(e, rows[i], y, xa, xb, cssW);
        break;
      case "stage":
        drawPill(e.stage, e.side === "a" ? xa : xb, y, theme.stroke);
        break;
      case "app":
        drawApp(e, e.side === "a" ? xa : xb, y);
        break;
      case "trace":
        drawTrace(e, y, xa, xb);
        break;
      case "lost":
        drawCentred("✕ " + e.side.toUpperCase() + ": " + e.cause, y, xa, xb, theme.bad);
        break;
      case "fault":
        drawBand(e.detail, y, GUTTER, cssW);
        break;
      case "note":
        drawCentred(e.text, y, xa, xb, theme.note);
        break;
      case "end":
        drawCentred(
          e.ok ? "run complete" : "run failed: " + e.error,
          y,
          xa,
          xb,
          e.ok ? theme.good : theme.bad,
        );
        break;
      default:
        break;
    }
  }

  // playhead
  const hy = Y(head);
  if (hy >= 0 && hy <= H) {
    ctx.strokeStyle = theme.stroke;
    ctx.globalAlpha = 0.5;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(0, hy);
    ctx.lineTo(cssW, hy);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
}

function drawPacket(e, rowY, y, xa, xb, cssW) {
  const from = e.from === "a" ? xa : xb;
  const to = e.from === "a" ? xb : xa;
  const p = clamp((head - rowY) / FLIGHT, 0, 1);
  const dead = e.fate === "lost" ? 0.5 : e.fate === "blackholed" ? 0.22 : 1;
  const q = Math.min(p, dead);
  const tip = from + (to - from) * q;

  const colour =
    e.fate === "lost"
      ? theme.bad
      : e.fate === "blackholed"
        ? theme.note
        : e.type === "init" || e.type === "resp"
          ? theme.stroke
          : theme.data;

  ctx.strokeStyle = colour;
  ctx.lineWidth = e.type === "init" || e.type === "resp" ? 2 : 1.4;
  ctx.setLineDash(e.copy > 0 ? [3, 3] : []);
  ctx.beginPath();
  ctx.moveTo(from, y);
  ctx.lineTo(tip, y);
  ctx.stroke();
  ctx.setLineDash([]);

  // the travelling dot, and what becomes of it
  if (p >= dead && dead < 1) {
    ctx.strokeStyle = theme.bad;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(tip - 4, y - 4);
    ctx.lineTo(tip + 4, y + 4);
    ctx.moveTo(tip + 4, y - 4);
    ctx.lineTo(tip - 4, y + 4);
    ctx.stroke();
  } else {
    ctx.fillStyle = colour;
    ctx.beginPath();
    ctx.arc(tip, y, p >= 1 ? 3 : 4.5, 0, Math.PI * 2);
    ctx.fill();
    if (p >= 1) {
      // arrowhead once it has landed
      const dir = Math.sign(to - from);
      ctx.beginPath();
      ctx.moveTo(tip, y);
      ctx.lineTo(tip - dir * 7, y - 4);
      ctx.lineTo(tip - dir * 7, y + 4);
      ctx.closePath();
      ctx.fill();
    }
  }

  const name =
    (e.type === "init"
      ? "msg1"
      : e.type === "resp"
        ? "msg2"
        : e.type) +
    " · " +
    e.len +
    "B" +
    (e.copy > 0 ? " (copy)" : "");
  ctx.fillStyle = theme.note;
  ctx.textAlign = e.from === "a" ? "left" : "right";
  ctx.fillText(name, from + (e.from === "a" ? 8 : -8), y - 9);
}

function drawPill(text, x, y, colour) {
  ctx.font = "11px " + theme.mono;
  const w = ctx.measureText(text).width + 14;
  ctx.fillStyle = theme.fill;
  ctx.strokeStyle = colour;
  ctx.lineWidth = 1;
  roundRect(x - w / 2, y - 9, w, 18, 9);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = theme.text;
  ctx.textAlign = "center";
  ctx.fillText(text, x, y);
}

function drawApp(e, x, y) {
  const text =
    e.op === "close"
      ? "close(" + e.code + ")"
      : e.op + (e.text !== undefined ? " “" + trunc(e.text, 28) + "”" : "");
  ctx.fillStyle = theme.good;
  ctx.textAlign = "center";
  ctx.fillText("▸ " + text, x, y);
}

function drawTrace(e, y, xa, xb) {
  const short = e.target.replace("slither::", "");
  const colour = short === "replay" ? theme.warn : theme.note;
  ctx.fillStyle = colour;
  ctx.textAlign = "center";
  ctx.fillText(short + " · " + trunc(e.message, 52), (xa + xb) / 2, y);
}

function drawCentred(text, y, xa, xb, colour) {
  ctx.fillStyle = colour;
  ctx.textAlign = "center";
  ctx.fillText(trunc(text, 78), (xa + xb) / 2, y);
}

function drawBand(text, y, x0, x1) {
  ctx.strokeStyle = theme.bad;
  ctx.setLineDash([5, 4]);
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(x0, y);
  ctx.lineTo(x1, y);
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.fillStyle = theme.bad;
  ctx.textAlign = "left";
  ctx.fillText(" " + trunc(text, 60), x0 + 6, y - 9);
}

function roundRect(x, y, w, h, r) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

// ── panels ─────────────────────────────────────────────────────────────

function syncPanels() {
  const idx = indexAt(head);
  if (idx === lastIndex) return;
  lastIndex = idx;

  const seen = events.slice(0, idx + 1);
  const badges = {};
  const counters = {
    datagrams: 0,
    lost: 0,
    dup: 0,
    retx: 0,
    dha: 0,
    dhb: 0,
    dead: {},
  };
  let vt = 0;
  let inits = 0;
  for (const e of seen) {
    vt = e.t_us;
    if (e.kind === "tx") {
      if (e.copy === 0) counters.datagrams += 1;
      if (e.fate === "lost") counters.lost += 1;
      if (e.copy > 0) counters.dup += 1;
      if (e.type === "init") inits += 1;
    } else if (e.kind === "stage") {
      badges[e.stage] = true;
      counters.dha = e.dhs.a;
      counters.dhb = e.dhs.b;
    } else if (e.kind === "lost") {
      counters.dead[e.side] = true;
    }
  }
  counters.retx = Math.max(0, inits - 1);

  for (const el of document.querySelectorAll(".badge")) {
    const stage = el.dataset.stage;
    el.classList.toggle("on", !!badges[stage]);
  }
  const deadA = counters.dead.a;
  const deadB = counters.dead.b;
  document
    .querySelectorAll("#ladder-a .badge")
    .forEach((el) => el.classList.toggle("dead", !!deadA));
  document
    .querySelectorAll("#ladder-b .badge")
    .forEach((el) => el.classList.toggle("dead", !!deadB));

  $("m-datagrams").textContent = counters.datagrams;
  $("m-lost").textContent = counters.lost;
  $("m-dup").textContent = counters.dup;
  $("m-dh").textContent = counters.dha + " / " + counters.dhb;
  $("m-retx").textContent = counters.retx;
  $("m-virtual").textContent = formatTime(vt);
  $("m-wall").textContent = (summary.wallMs ?? 0) + " ms";
  $("vtime").textContent = formatTime(vt);

  renderLog(idx);
}

function renderLog(idx) {
  const log = $("log");
  if (log.childElementCount !== events.length) {
    log.textContent = "";
    for (const e of events) {
      const div = document.createElement("div");
      const t = document.createElement("span");
      t.className = "t";
      t.textContent = formatTime(e.t_us);
      div.appendChild(t);
      div.appendChild(document.createTextNode(describe(e)));
      if (e.kind === "lost" || (e.kind === "tx" && e.fate === "lost")) {
        div.classList.add("lost");
      } else if (e.kind === "trace") {
        div.classList.add("warn");
      } else if (e.kind === "app" || e.kind === "stage") {
        div.classList.add("good");
      }
      log.appendChild(div);
    }
  }
  const kids = log.children;
  for (let i = 0; i < kids.length; i++) {
    kids[i].classList.toggle("now", i === idx);
  }
  if (idx >= 0 && kids[idx]) {
    kids[idx].scrollIntoView({ block: "nearest" });
  }
}

function describe(e) {
  switch (e.kind) {
    case "run":
      return `run ${e.scenario} seed=${e.seed} loss=${e.loss} dup=${e.duplicate}`;
    case "topology":
      return `a=${e.a} b=${e.b}`;
    case "stage":
      return `${e.side}: ${e.stage}   dh a=${e.dhs.a} b=${e.dhs.b}`;
    case "tx":
      return `${e.from}→${e.to}  ${e.type} ${e.len}B  ${e.fate}${e.copy ? " (copy)" : ""}`;
    case "trace":
      return `${e.target}  ${e.message}${fieldsOf(e)}`;
    case "app":
      return `${e.side}: ${e.op}${e.text !== undefined ? ' "' + e.text + '"' : ""}`;
    case "lost":
      return `${e.side}: connection lost — ${e.cause}`;
    case "fault":
      return `network: ${e.what} — ${e.detail}`;
    case "note":
      return e.text;
    case "stderr":
      return `stderr: ${e.text}`;
    case "end":
      return e.ok
        ? `end — ${e.sent} sent, ${e.lost} lost, ${e.duplicated} duplicated, ${e.blackholed} blackholed, ${e.wall_us / 1000} ms wall`
        : `end — FAILED: ${e.error}`;
    case "error":
      return `error: ${e.message}`;
    default:
      return e.kind;
  }
}

function fieldsOf(e) {
  const keys = Object.keys(e.fields || {});
  if (!keys.length) return "";
  return "  " + keys.map((k) => `${k}=${e.fields[k]}`).join(" ");
}

// ── controls ───────────────────────────────────────────────────────────

function wireControls() {
  $("blurb").textContent = BLURBS[scenario];

  for (const btn of document.querySelectorAll("#scenarios button")) {
    btn.addEventListener("click", () => {
      scenario = btn.dataset.scenario;
      for (const other of document.querySelectorAll("#scenarios button")) {
        other.setAttribute("aria-pressed", String(other === btn));
      }
      $("blurb").textContent = BLURBS[scenario];
      applyPreset(scenario);
      run();
    });
  }

  const bind = (el, out, fmt) => {
    const update = () => {
      out.textContent = fmt(Number(el.value));
    };
    el.addEventListener("input", () => {
      update();
      schedule();
    });
    update();
  };
  bind(controls.loss, $("loss-out"), (v) => v + "%");
  bind(controls.duplicate, $("duplicate-out"), (v) => v + "%");
  bind(controls.delay, $("delay-out"), (v) => v + " ms");
  bind(controls.jitter, $("jitter-out"), (v) => v + " ms");
  controls.seed.addEventListener("input", schedule);
  controls.message.addEventListener("input", schedule);
  $("reseed").addEventListener("click", () => {
    controls.seed.value = String(Math.floor(Math.random() * 100000));
    run();
  });

  $("play").addEventListener("click", () => {
    if (head >= contentHeight) head = 0;
    playing = !playing;
    lastFrame = performance.now();
    $("play").textContent = playing ? "Pause" : "Play";
  });
  $("scrub").addEventListener("input", (ev) => {
    playing = false;
    $("play").textContent = "Play";
    head = (Number(ev.target.value) / 1000) * contentHeight;
  });

  if (window.matchMedia) {
    window
      .matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", () => {
        theme = readTheme();
      });
  }
}

/** Each scenario arrives with the settings that make it show its point. */
function applyPreset(name) {
  const preset = {
    clean: { loss: 0, duplicate: 0, delay: 0, jitter: 0, seed: 6 },
    lossy: { loss: 35, duplicate: 0, delay: 0, jitter: 0, seed: 6 },
    duplicate: { loss: 0, duplicate: 60, delay: 0, jitter: 0, seed: 3 },
    partition: { loss: 0, duplicate: 0, delay: 0, jitter: 0, seed: 5 },
  }[name];
  if (!preset) return;
  controls.loss.value = preset.loss;
  controls.duplicate.value = preset.duplicate;
  controls.delay.value = preset.delay;
  controls.jitter.value = preset.jitter;
  controls.seed.value = preset.seed;
  $("loss-out").textContent = preset.loss + "%";
  $("duplicate-out").textContent = preset.duplicate + "%";
  $("delay-out").textContent = preset.delay + " ms";
  $("jitter-out").textContent = preset.jitter + " ms";
}

// ── helpers ────────────────────────────────────────────────────────────

function indexAt(y) {
  let lo = 0;
  let hi = rows.length - 1;
  let best = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid] <= y) {
      best = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

function formatTime(us) {
  const s = (us || 0) / 1e6;
  return s >= 1 ? s.toFixed(3) + " s" : (us / 1000).toFixed(1) + " ms";
}

function trunc(s, n) {
  s = String(s ?? "");
  return s.length <= n ? s : s.slice(0, n - 1) + "…";
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

function setStatus(text, isError) {
  statusEl.textContent = text;
  statusEl.classList.toggle("err", !!isError);
}

function fatal(text) {
  setStatus(text, true);
}

function readTheme() {
  const css = getComputedStyle(document.documentElement);
  const v = (n, fallback) => (css.getPropertyValue(n) || fallback).trim();
  const stroke = v("--stroke", "#0969da");
  return {
    stroke,
    fill: v("--fill", "#ddeafd"),
    text: v("--text", "#24292f"),
    note: v("--note", "#57606a"),
    line: v("--line", "#d0d7de"),
    bad: v("--bad", "#cf222e"),
    warn: v("--warn", "#9a6700"),
    good: v("--good", "#1a7f37"),
    data: v("--note", "#57606a"),
    mono: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
  };
}

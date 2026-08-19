// Static checks on the web half, for the things a browser would only tell
// you at runtime.
//
//   node demo/verify/check-web.mjs
//
// 1. Every JavaScript module parses as an ES module.
// 2. Every local asset `index.html` and the modules reference exists.
// 3. **Nothing reaches a third-party host.** GitHub Pages serves this
//    page, and a CDN import would make the demo depend on somebody else's
//    uptime and integrity. The only permitted absolute URL is the repo
//    link in the page's prose.
// 4. The vendored shim's licence files are present, because redistributing
//    it without them is a licence violation.

import { readFile, readdir, access } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { writeFile, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { exit } from "node:process";

const web = fileURLToPath(new URL("../web/", import.meta.url));
let failures = 0;
const fail = (m) => {
  console.log(`  FAIL  ${m}`);
  failures += 1;
};
const pass = (m) => console.log(`  ok    ${m}`);

// ── 1. every module parses ─────────────────────────────────────────────
console.log("\nmodule syntax");
const tmp = await mkdtemp(join(tmpdir(), "slither-demo-"));
const jsFiles = [];
for (const entry of await readdir(web, { withFileTypes: true, recursive: true })) {
  if (entry.isFile() && entry.name.endsWith(".js")) {
    jsFiles.push(join(entry.parentPath ?? entry.path, entry.name));
  }
}
for (const file of jsFiles) {
  // `node --check` decides module-vs-script from the extension, and these
  // are `.js` with no package.json beside them, so it would read them as
  // CommonJS and reject every `import`. Copying to `.mjs` is the whole
  // trick.
  const copy = join(tmp, Math.random().toString(36).slice(2) + ".mjs");
  await writeFile(copy, await readFile(file));
  const r = spawnSync(process.execPath, ["--check", copy], {
    encoding: "utf8",
  });
  if (r.status !== 0) fail(`${rel(file)}: ${r.stderr.trim().split("\n")[0]}`);
}
if (failures === 0) pass(`${jsFiles.length} modules parse`);

// ── 2 & 3. references ──────────────────────────────────────────────────
console.log("\nreferences");
const html = await readFile(join(web, "index.html"), "utf8");
const sources = new Map([["index.html", html]]);
for (const file of jsFiles) sources.set(rel(file), await readFile(file, "utf8"));
sources.set("style.css", await readFile(join(web, "style.css"), "utf8"));

const localRefs = new Set();
for (const [name, text] of sources) {
  // href/src/url() and bare module specifiers starting with ./ or ../
  for (const m of text.matchAll(/(?:href|src)="([^"]+)"/g)) collect(name, m[1]);
  for (const m of text.matchAll(/from\s*"([^"]+)"/g)) collect(name, m[1]);
  for (const m of text.matchAll(/new URL\(\s*"([^"]+)"/g)) collect(name, m[1]);
  for (const m of text.matchAll(/fetch\(\s*"([^"]+)"/g)) collect(name, m[1]);
}

function collect(where, ref) {
  if (/^(https?:)?\/\//.test(ref)) {
    // The one allowed absolute URL is the repo link in the page's prose.
    if (where === "index.html" && /github\.com/.test(ref)) return;
    fail(`${where} reaches an external host: ${ref}`);
    return;
  }
  if (ref.startsWith("#") || ref.startsWith("data:")) return;
  localRefs.add(ref.replace(/^\.\//, ""));
}

for (const ref of localRefs) {
  const candidates = [
    join(web, ref),
    join(web, "vendor/browser_wasi_shim", ref.replace(/^.*\//, "")),
  ];
  let found = false;
  for (const c of candidates) {
    try {
      await access(c);
      found = true;
      break;
    } catch {
      /* try the next */
    }
  }
  // `slither-demo.wasm` is a build artefact assembled by build.sh, not a
  // file in the tree.
  if (!found && ref !== "slither-demo.wasm") fail(`missing asset: ${ref}`);
}
pass(`${localRefs.size} local references, no external hosts`);

// ── 4. vendored licences ───────────────────────────────────────────────
console.log("\nvendored shim");
for (const f of ["LICENSE-MIT", "LICENSE-APACHE", "PROVENANCE.md", "wasi.js"]) {
  try {
    await access(join(web, "vendor/browser_wasi_shim", f));
    pass(`vendor/browser_wasi_shim/${f}`);
  } catch {
    fail(`vendor/browser_wasi_shim/${f} is missing`);
  }
}

// The demo's whole browser story rests on this one line of the shim being
// what we think it is. If upstream ever makes `poll_oneoff` block properly
// the paused-clock argument stops being load-bearing and this note should
// be revisited — so assert the shape rather than trusting PROVENANCE.md.
const wasiSrc = await readFile(
  join(web, "vendor/browser_wasi_shim/wasi.js"),
  "utf8",
);
if (/poll_oneoff/.test(wasiSrc) && /while\(endTime>getNow\(\)\)\{\}/.test(wasiSrc))
  pass("poll_oneoff is still the busy-wait spin PROVENANCE.md describes");
else fail("poll_oneoff no longer matches PROVENANCE.md — re-read the shim");

function rel(p) {
  return p.slice(web.length);
}

console.log(failures === 0 ? "\nALL CHECKS PASSED" : `\n${failures} CHECK(S) FAILED`);
exit(failures === 0 ? 0 : 1);

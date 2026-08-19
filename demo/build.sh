#!/usr/bin/env bash
# Assemble the static site into demo/_site/.
#
# Identical to what .github/workflows/pages.yml runs, so a local
# `demo/build.sh && python3 -m http.server -d demo/_site` is the same
# artefact GitHub Pages serves. Keep the two in step.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/_site"
wasm="$here/runner/target/wasm32-wasip1/release/slither-demo.wasm"

echo "==> cargo build --release --target wasm32-wasip1"
(cd "$here/runner" && cargo build --release --target wasm32-wasip1)

echo "==> assembling $out"
rm -rf "$out"
mkdir -p "$out"
cp -R "$here/web/." "$out/"
cp "$wasm" "$out/slither-demo.wasm"

# Nothing in `web/` should reach the site except the page itself; the
# vendored shim's licences do, deliberately, because we are redistributing
# it.
size=$(wc -c < "$out/slither-demo.wasm" | tr -d ' ')
echo "==> $out ready ($(printf '%d' $((size / 1024))) KB of wasm)"
find "$out" -type f | sed "s|$out|  _site|" | sort

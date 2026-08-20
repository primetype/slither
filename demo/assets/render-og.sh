#!/usr/bin/env bash
# Render demo/web/og.png (1200x630) from demo/assets/og.html.
#
# A MANUAL step, run when the card changes; og.png is committed. The card
# uses the rounded system face, so render it on macOS (SF Pro Rounded) for
# the canonical output.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
"$chrome" --headless --disable-gpu --hide-scrollbars \
  --force-device-scale-factor=1 --window-size=1200,630 \
  --screenshot="$here/../web/og.png" "file://$here/og.html"
echo "wrote demo/web/og.png ($(wc -c < "$here/../web/og.png" | tr -d ' ') bytes)"

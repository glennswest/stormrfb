#!/usr/bin/env bash
# Is every slide of docs/presentation.md inside its 16:9 page? (#11)
#
#   sc-build tools/check-slides.sh
#
# Marp renders the deck to HTML with the bare template; headless Chromium
# (Playwright) measures each slide and prints it as a PNG line in the log.
# Exit 1 if any slide overflows. Nothing is kept.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/slides.XXXXXX")
trap 'rm -rf "$W"' EXIT

echo "=== Marp and Playwright"
(cd "$W" && npm init -y >/dev/null && npm i --no-audit --no-fund @marp-team/marp-cli playwright@1 >/dev/null 2>&1 \
  && npx playwright install chromium-headless-shell >/dev/null 2>&1)
echo "  marp-cli $(node -p "require('$W/node_modules/@marp-team/marp-cli/package.json').version"), playwright $(node -p "require('$W/node_modules/playwright/package.json').version")"

echo "=== render docs/presentation.md"
(cd "$W" && npx marp --template bare --html -o "$W/deck.html" "$HERE/../docs/presentation.md" 2>&1 | sed 's/^/  /')

echo "=== measure each slide"
cp "$HERE/slides.browser.cjs" "$W/"
(cd "$W" && DECK="$W/deck.html" node slides.browser.cjs)

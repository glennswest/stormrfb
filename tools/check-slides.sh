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
# Each step says why when it fails; quiet when it works.
step() { local log="$W/step.log"; if ! "$@" > "$log" 2>&1; then echo "  FAILED: $*"; tail -30 "$log" | sed 's/^/    /'; exit 1; fi; }
cd "$W"
step npm init -y
step npm i --no-audit --no-fund playwright@1
step npx playwright install chromium-headless-shell
# Marp's own browser is not needed for HTML; skip any download it attempts.
PUPPETEER_SKIP_DOWNLOAD=1 step npm i --no-audit --no-fund @marp-team/marp-cli
cd - >/dev/null
echo "  marp-cli $(node -p "require('$W/node_modules/@marp-team/marp-cli/package.json').version"), playwright $(node -p "require('$W/node_modules/playwright/package.json').version")"

echo "=== render docs/presentation.md"
(cd "$W" && npx marp --template bare --html -o "$W/deck.html" "$HERE/../docs/presentation.md" 2>&1 | sed 's/^/  /')
[ -s "$W/deck.html" ] || { echo "  Marp wrote no deck.html"; exit 1; }

echo "=== measure each slide"
cp "$HERE/slides.browser.cjs" "$W/"
(cd "$W" && DECK="$W/deck.html" node slides.browser.cjs)

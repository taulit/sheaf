#!/usr/bin/env bash
# Renders the app offscreen and writes the PNGs to docs/screenshots/app-*.png.
# No window is opened. Needs a GPU or a software Vulkan driver (lavapipe).
#
#   scripts/screenshot.sh               all scenes, dark and light
#   scripts/screenshot.sh app_scenes    only the tests matching the filter
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo test -p sheaf --test screenshots -- --ignored --nocapture --test-threads=1 "$@"

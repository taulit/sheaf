#!/usr/bin/env bash
# Records the demo screencast of Sheaf in the sandboxed headless GNOME Shell of
# scripts/shell-sandbox.sh. Nothing touches the desktop you are logged in to.
#
#   cargo build --release --workspace
#   marketing/video/record/record.sh [light|dark]
#
# Writes marketing/video/public/recording-<scheme>.mp4 (30 fps, 2880x1620, a 1440x810
# desktop at scale 2), recording-<scheme>.json with the time of each scene, and
# stills in marketing/video/public/stills/.
set -euo pipefail
repo="$(cd "$(dirname "$0")/../../.." && pwd)"
scheme="${1:-light}"
export SHEAF_BINDIR="${SHEAF_BINDIR:-$repo/target/release}"
DEMO_OUT="$repo/marketing/video/public"
mkdir -p "$DEMO_OUT/stills"
"$repo/scripts/shell-sandbox.sh" --size 2880x1620 --scheme "$scheme" \
    --log "$repo/target/demo-shell-$scheme.log" -- "$repo/marketing/video/record/scene.sh"
raw="$DEMO_OUT/recording-$scheme.raw.mp4"
# The shell records frames only when something changes; make it constant 30 fps.
ffmpeg -loglevel error -y -i "$raw" -vf fps=30 -c:v libx264 -preset slow -crf 14 \
    -pix_fmt yuv420p -movflags +faststart "$DEMO_OUT/recording-$scheme.mp4"
rm -f "$raw"
echo "wrote $DEMO_OUT/recording-$scheme.mp4"

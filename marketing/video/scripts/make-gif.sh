#!/usr/bin/env bash
# Turns out/sheaf-gif-source.mp4 (pnpm render:gif-source) into the README GIF.
set -euo pipefail
cd "$(dirname "$0")/.."
src=out/sheaf-gif-source.mp4
dst=../../docs/screenshots/sheaf-demo.gif
filters="fps=15,scale=800:-1:flags=lanczos"
ffmpeg -loglevel error -y -i "$src" -vf "$filters,palettegen=max_colors=192:stats_mode=diff" /tmp/sheaf-gif-palette.png
ffmpeg -loglevel error -y -i "$src" -i /tmp/sheaf-gif-palette.png \
    -lavfi "$filters [x]; [x][1:v] paletteuse=dither=sierra2_4a:diff_mode=rectangle" -loop 0 "$dst"
rm -f /tmp/sheaf-gif-palette.png
echo "wrote $dst ($(du -h "$dst" | cut -f1))"

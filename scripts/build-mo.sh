#!/bin/sh
# Compiles po/*.po to <dir>/<lang>/LC_MESSAGES/sheaf.mo (default: target/locale).
# Run a development build with it: SHEAF_LOCALEDIR=target/locale LANGUAGE=de sheaf list
set -eu
cd "$(dirname "$0")/.."
out="${1:-target/locale}"

for po in po/*.po; do
    lang="$(basename "$po" .po)"
    mkdir -p "$out/$lang/LC_MESSAGES"
    msgfmt --check --output-file="$out/$lang/LC_MESSAGES/sheaf.mo" "$po"
done

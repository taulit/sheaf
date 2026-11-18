#!/usr/bin/env bash
# Builds the extensions.gnome.org zip of the Visor module into target/ (or the
# directory given as the first argument) and prints its path.
# Uses `gnome-extensions pack` when it is installed, else zip and msgfmt.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$repo/visor-module"
out="$(realpath -m "${1:-$repo/target}")"
uuid="$(jq -r .uuid "$src/metadata.json")"
domain="$(jq -r '."gettext-domain"' "$src/metadata.json")"
zip="$out/$uuid.shell-extension.zip"
mkdir -p "$out"

if command -v gnome-extensions >/dev/null; then
    gnome-extensions pack --force --out-dir "$out" \
        --podir=po --gettext-domain="$domain" \
        --extra-source=lib --extra-source="$repo/LICENSE" "$src" >&2
else
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    cp -r "$src/extension.js" "$src/metadata.json" "$src/stylesheet.css" "$src/lib" "$repo/LICENSE" "$stage/"
    for po in "$src"/po/*.po; do
        lang="$(basename "$po" .po)"
        mkdir -p "$stage/locale/$lang/LC_MESSAGES"
        msgfmt --check --output-file="$stage/locale/$lang/LC_MESSAGES/$domain.mo" "$po"
    done
    rm -f "$zip"
    (cd "$stage" && zip -qr "$zip" .)
fi
echo "$zip"

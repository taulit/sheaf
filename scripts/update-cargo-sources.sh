#!/bin/sh
# Regenerates build-aux/flatpak/cargo-sources.json from Cargo.lock with flatpak-cargo-generator
# (flatpak/flatpak-builder-tools, pinned below). Run it again whenever Cargo.lock changes.
#
# The generator and its two Python packages (aiohttp, tomlkit) go into a temporary directory
# and are removed afterwards; nothing is installed on the system. The generator clones the git
# dependency taulit/gpui-markdown-editor, so git needs credentials for it while that
# repository is private. Needs network, python3 (with venv and pip), git, curl.
set -eu

# Pinned: flatpak-builder-tools at this commit, cargo/flatpak-cargo-generator.py with this sha256.
tools_commit="9d86d336a39b9af38b86f114b6ce5820fe260f91"
generator_sha256="0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380"
url="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$tools_commit/cargo/flatpak-cargo-generator.py"

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/build-aux/flatpak/cargo-sources.json"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

curl -fsSL "$url" -o "$work/flatpak-cargo-generator.py"
echo "$generator_sha256  $work/flatpak-cargo-generator.py" | sha256sum -c --quiet - ||
    { echo "$0: the generator does not match the pinned checksum" >&2; exit 1; }

python3 -m venv "$work/venv"
"$work/venv/bin/pip" install --quiet --disable-pip-version-check "aiohttp>=3.9.5,<4" "tomlkit>=0.13.3,<1"

mkdir -p "$(dirname "$out")"
"$work/venv/bin/python" -I "$work/flatpak-cargo-generator.py" "$root/Cargo.lock" -o "$work/cargo-sources.json"
mv "$work/cargo-sources.json" "$out"
echo "wrote $out ($(jq length "$out") sources)"

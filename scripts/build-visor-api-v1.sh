#!/usr/bin/env bash
# Builds Visor at the git tag api-v1 from a local checkout of taulit/visor into
# target/visor-api-v1/build and prints that directory. The checkout is only read.
#
# Usage: scripts/build-visor-api-v1.sh [VISOR_CHECKOUT]   (default: $VISOR_REPO, else ~/development/visor)
# Needs the checkout's node_modules (pnpm install there), for tsc.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
visor_repo="${1:-${VISOR_REPO:-$HOME/development/visor}}"
tag=api-v1
work="$repo/target/visor-api-v1"

commit="$(git -C "$visor_repo" rev-parse --verify --quiet "$tag^{commit}")" || {
    echo "build-visor-api-v1.sh: no tag $tag in $visor_repo; clone taulit/visor and pass its path" >&2
    exit 1
}
[[ -x "$visor_repo/node_modules/.bin/tsc" ]] || {
    echo "build-visor-api-v1.sh: $visor_repo/node_modules is missing, run 'pnpm install' there" >&2
    exit 1
}

if [[ "$(cat "$work/commit" 2>/dev/null)" != "$commit" ]]; then
    rm -rf "$work"
    mkdir -p "$work/src"
    git -C "$visor_repo" archive "$tag" | tar -x -C "$work/src"
    ln -s "$visor_repo/node_modules" "$work/src/node_modules"
    (cd "$work/src" && PATH="$PWD/node_modules/.bin:$PATH" bash scripts/build.sh) >&2
    mv "$work/src/build" "$work/build"
    echo "$commit" >"$work/commit"
fi
echo "$work/build"

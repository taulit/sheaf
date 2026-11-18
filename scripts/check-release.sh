#!/bin/sh
# Checks that the files a release touches agree on one version, before a tag is pushed and
# again in the release workflow.   scripts/check-release.sh 0.1.0
set -eu
[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
v="$1"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
fail=0
bad() { echo "check-release: $*" >&2; fail=1; }

cargo_v="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml)"
[ "$cargo_v" = "$v" ] || bad "Cargo.toml says version $cargo_v, not $v"

date="$(sed -n "s/^## \[$v\] - \([0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}\)\$/\1/p" CHANGELOG.md)"
[ -n "$date" ] || bad "CHANGELOG.md has no section '## [$v] - YYYY-MM-DD'"

meta=data/io.github.taulit.Sheaf.metainfo.xml
grep -q "<release version=\"$v\" date=\"${date:-none}\">" "$meta" ||
    bad "$meta has no <release version=\"$v\" date=\"$date\"> (the date must equal the changelog's)"
grep -q "/v$v/docs/screenshots/" "$meta" || bad "$meta: screenshot URLs do not point at the tag v$v"
if grep 'raw.githubusercontent.com' "$meta" | grep -vq "/v$v/"; then
    bad "$meta: a screenshot URL points at another tag"
fi

module_v="$(jq -r '."version-name"' visor-module/metadata.json)"
[ "$module_v" = "$v" ] || bad "visor-module/metadata.json says version-name $module_v, not $v"

aur_v="$(sed -n 's/^pkgver=//p' build-aux/aur/sheaf/PKGBUILD)"
[ "$aur_v" = "$v" ] || bad "build-aux/aur/sheaf/PKGBUILD says pkgver $aur_v, not $v"
grep -q "^  *tag: v$v\$" build-aux/flatpak/io.github.taulit.Sheaf.yml ||
    bad "build-aux/flatpak/io.github.taulit.Sheaf.yml does not name tag v$v"

[ "$fail" -eq 0 ] || exit 1
echo "check-release: version $v is consistent (changelog date $date)"

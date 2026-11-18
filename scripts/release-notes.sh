#!/bin/sh
# Prints the notes of one release: the body of its section in CHANGELOG.md, without the heading.
#   scripts/release-notes.sh 0.1.0
set -eu
[ $# -eq 1 ] || { echo "usage: $0 <version>" >&2; exit 2; }
root="$(cd "$(dirname "$0")/.." && pwd)"
notes="$(awk -v v="$1" '
    $0 ~ "^## \\[" v "\\]" { on = 1; next }
    on && /^## \[/ { exit }
    on && /^\[[^]]+\]: / { exit }
    on { print }
' "$root/CHANGELOG.md")"
# Trim blank lines at both ends.
notes="$(printf '%s\n' "$notes" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"
[ -n "$notes" ] || { echo "$0: no section for version $1 in CHANGELOG.md" >&2; exit 1; }
printf '%s\n' "$notes"

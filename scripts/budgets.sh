#!/usr/bin/env bash
# Measures the performance budgets of plan 05 §1 on a release build and prints one
# table. Exits non-zero when a budget fails. How each number is taken, and what it
# leaves out, is in docs/testing.md, "Budgets".
#
# Usage: scripts/budgets.sh [RUNS]
#   RUNS  starts of the app to measure (default 9); the first one is reported apart
#
# Nothing here touches your notes, settings or session: every measurement runs under
# `env -i` with its own HOME and XDG directories in a temporary folder, generated
# notes in SHEAF_NOTES_DIR, and a D-Bus session of its own.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs="${1:-9}"
bin="$repo/target/release"
# sheaf-searchd as it ships: built with the profile `service` (Cargo.toml).
svc="$repo/target/service"

cd "$repo"
echo "building (release, --locked) ..." >&2
cargo build --release --locked --quiet -p sheaf
cargo build --profile service --locked --quiet -p sheaf-searchd
cargo build --release --locked --quiet -p sheaf --features budgets --bin sheaf-budgets
cargo build --release --locked --quiet -p sheaf-core --features testgen --example search_budget

work="$(mktemp -d "${TMPDIR:-/tmp}/sheaf-budgets.XXXXXX")"
trap 'rm -rf "$work"' EXIT
rows="$work/rows"
: >"$rows"

home="$work/home"
services="$home/.local/share/dbus-1/services"
mkdir -p "$services" "$home/.config" "$home/.cache" "$home/.local/state" "$work/run"
chmod 700 "$work/run"
sed "s|@bindir@|$svc|g" data/io.github.taulit.Sheaf.SearchProvider.service.in \
    >"$services/io.github.taulit.Sheaf.SearchProvider.service"
# Only sheaf-searchd can be started on the bus: no portal or other desktop service.
cat >"$work/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$work/run</listen>
  <auth>EXTERNAL</auth>
  <servicedir>$services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF

# isolated NOTES [bus] COMMAND...: runs COMMAND in a fresh environment that knows only
# the temporary folder; with `bus`, on a session bus of its own.
isolated() {
    local notes="$1"
    shift
    if [[ "$1" == bus ]]; then
        shift
        set -- dbus-run-session --config-file="$work/bus.conf" -- "$@"
    fi
    env -i PATH="$PATH" HOME="$home" LANG=en_US.UTF-8 TZ=Europe/Berlin \
        XDG_RUNTIME_DIR="$work/run" XDG_CONFIG_HOME="$home/.config" \
        XDG_DATA_HOME="$home/.local/share" XDG_DATA_DIRS="$home/.local/share" \
        XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state" \
        SHEAF_NOTES_DIR="$notes" "$@"
}

echo "generating notes ..." >&2
"$bin/sheaf-budgets" generate "$work/notes-1k" 1000 15
"$bin/sheaf-budgets" generate "$work/notes-10k" 10000 15
# The app reads the index sheaf-searchd wrote, as after a login.
isolated "$work/notes-1k" "$svc/sheaf-searchd" --reindex >/dev/null
isolated "$work/notes-10k" "$svc/sheaf-searchd" --reindex >/dev/null

status=0
echo "measuring the start, $runs runs ..." >&2
isolated "$work/notes-1k" bus "$bin/sheaf-budgets" start "$bin/sheaf" "$runs" >>"$rows" || status=1

echo "measuring the search ..." >&2
search="$("$bin/examples/search_budget")" || status=1
p95="$(awk '$1 == "all" { print $4 }' <<<"$search")"
if awk -v p95="$p95" 'BEGIN { exit !(p95 < 30) }'; then verdict=pass; else verdict=FAIL; status=1; fi
printf 'search p95, 10,000 notes\t< 30 ms\t%s ms (query mix of testgen::QUERIES)\t%s\n' \
    "$p95" "$verdict" >>"$rows"

echo "measuring scrolling ..." >&2
isolated "$work/notes-10k" "$bin/sheaf-budgets" scroll "$work/notes-10k" >>"$rows" || status=1

echo "measuring sheaf-searchd ..." >&2
isolated "$work/notes-1k" bus "$bin/sheaf-budgets" searchd "$svc/sheaf-searchd" 1,000 \
    >>"$rows" || status=1
isolated "$work/notes-10k" bus "$bin/sheaf-budgets" searchd "$svc/sheaf-searchd" 10,000 \
    >>"$rows" || status=1

echo
printf '%s, %s, %s\n' "$(date -I)" "$(git rev-parse --short HEAD)" \
    "$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ *//')"
awk -F'\t' '
    BEGIN { printf "%-40s  %-28s  %-6s  %s\n", "budget", "target", "result", "measured" }
    { printf "%-40s  %-28s  %-6s  %s\n", $1, $2, $4, $3 }
' "$rows"
exit "$status"

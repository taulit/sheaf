#!/usr/bin/env bash
# Proves that GNOME Shell's overview search finds notes through Sheaf's search
# provider, inside the sandboxed headless shell of scripts/shell-sandbox.sh.
#
# Usage: scripts/shell-search-check.sh [--scheme dark|light] [--size WxH] [QUERY]
# QUERY defaults to "Milch". The screenshot goes to docs/screenshots/gnome-search-<scheme>.png,
# the shell log to .shell-test/search-<scheme>.log.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_id=io.github.taulit.Sheaf
expected_title="Milch kaufen"

if [[ "${1:-}" == --inside ]]; then
    source "$repo/scripts/lib/shell.sh"
    query="$2" out="$3" log="$4"
    fail() { printf 'FAIL: %s\n       shell log: %s\n' "$1" "$log" >&2; exit 1; }
    note() { sheaf new "$@" >/dev/null; }

    note --folder Schnellnotizen "Milch kaufen
Hafermilch und Butter nicht vergessen."
    note "Einkäufe für Samstag

- [ ] Brot
- [ ] Käse
- [ ] Tomaten
- [x] Eier"
    note --folder Arbeit "Ausschreibung Rathaus

Frist für das Angebot: 28. November. Unterlagen vom Bauamt abholen. #arbeit"
    note --folder Arbeit "Quarterly planning

Agenda: roadmap, hiring, budget review. #meeting"
    note "Pfannkuchen Rezept

250 g Mehl, 3 Eier, 500 ml Milch, eine Prise Salz."
    note "Trip to Lisbon

Flights on Friday, hotel near Alfama, pastéis de nata on day one."
    note "Reading list

- [ ] The Pragmatic Programmer
- [ ] Designing Data-Intensive Applications"
    note "WLAN-Passwort Gäste

Router im Flur, Passwort steht auf dem Aufkleber unten."

    # Names of the results the overview shows for Sheaf, or null while there are none.
    provider_js='Main.overview.searchController._searchResults._providers.find(p => p.id === "'"$app_id"'.desktop")'
    names_js="(() => { const p = $provider_js; return p ? Object.values(p.display._resultDisplays).map(d => d.metaInfo.name) : null; })()"

    shell_eval 'Main.overview.show()' >/dev/null
    shell_wait_for 'Main.overview.visible' || fail "the overview did not open"
    shell_eval "Main.overview.searchEntry.set_text($(jq -Rn --arg q "$query" '$q'))" >/dev/null

    # The first search starts sheaf-searchd through D-Bus activation and builds the index.
    names=null
    if ! shell_wait_for "$names_js?.includes('$expected_title') ?? false" 30; then
        names="$(shell_eval "$names_js" 2>/dev/null || echo '?')"
        fail "the overview search has no \"$expected_title\" for \"$query\"; Sheaf results: $names"
    fi
    names="$(shell_eval "$names_js")"
    # Let the result row finish drawing before the screenshot.
    shell_wait_for "$provider_js.display.get_stage() !== null && $provider_js.display.mapped" 5 || true
    sleep 0.5
    shell_shot "$out"
    echo "overview results of Sheaf for \"$query\": $names"

    # The CLI talks to the live service: Sync() over D-Bus, then the on-disk index.
    sheaf new "Zahnarzt Termin verschieben" >/dev/null
    hits="$(sheaf search zahnarzt --json)"
    grep -q '"Zahnarzt Termin verschieben"' <<<"$hits" ||
        fail "sheaf search zahnarzt did not return the note created just before: $hits"
    echo "CLI search hit: $(grep -o '"title": *"Zahnarzt[^"]*"' <<<"$hits" | head -1)"

    owner="$(busctl --user status "$app_id.SearchProvider" 2>&1)" ||
        fail "$app_id.SearchProvider is not owned on the session bus: $owner"
    echo "bus name owned: $(grep -m1 '^PID=' <<<"$owner" | tr -d '\n') $app_id.SearchProvider"
    exit 0
fi

scheme=dark size=1280x800 query=Milch
while [[ $# -gt 0 ]]; do
    case "$1" in
        --scheme) scheme="$2"; shift 2 ;;
        --size) size="$2"; shift 2 ;;
        *) query="$1"; shift ;;
    esac
done
out="$repo/docs/screenshots/gnome-search-$scheme.png"
log="$repo/.shell-test/search-$scheme.log"

start=$SECONDS
status=0
"$repo/scripts/shell-sandbox.sh" --scheme "$scheme" --size "$size" --log "$log" -- \
    "$repo/scripts/shell-search-check.sh" --inside "$query" "$out" "$log" || status=$?
if ((status == 0)); then
    echo "PASS ($scheme, $((SECONDS - start)) s): $out"
else
    echo "FAIL ($scheme): shell log $log" >&2
fi
exit "$status"

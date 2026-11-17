#!/usr/bin/env bash
# Runs the end-to-end scenarios of plan 05 §3 that involve Sheaf and can run without a
# visible desktop, and prints pass or fail for each. Which scenarios these are, and what
# is left to a person at a real desktop, is in docs/testing.md, "End-to-end scenarios".
#
# Usage: scripts/e2e.sh [--visor DIR]
#   --visor DIR  a built Visor (default: scripts/build-visor-api-v1.sh)
#
# 5 "Notiz anlegen": in a sandboxed headless GNOME Shell with Visor, the module, the
#   real sheaf-searchd and the real app (scripts/visor-sandbox.sh), `notiz Milch kaufen`
#   in Visor's search saves a note, the notch confirms it, the file is in the Quick
#   Notes folder, the app's first gallery frame shows it (startup trace, app without a
#   display), and the overview search finds "Milch" under the app's provider.
# 6 "Externe Änderung": `sed` changes an open checklist note; it reloads silently, and
#   with unsaved edits the banner asks (crates/sheaf/tests/e2e.rs, with a private bus
#   and the real sheaf-searchd).
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${1:-}" == --inside ]]; then
    source "$repo/scripts/lib/shell.sh"
    helpers="$(<"$repo/visor-module/tests/shell-helpers.js")"
    failures=0
    pass() { printf '  ok    %s\n' "$1"; }
    fail() { printf '  FAIL  %s\n' "$1"; failures=$((failures + 1)); }
    step() { if shell_wait_for "$2" "${3:-10}" 2>/dev/null; then pass "$1"; else fail "$1: $(shell_eval "$2" 2>&1 | head -c 300)"; fi; }

    gsettings set org.gnome.shell enabled-extensions "$SHEAF_ENABLED"
    step "Visor and the module are active" \
        "Main.extensionManager.lookup('visor@taulit.github.io')?.state === 1 && Main.extensionManager.lookup('sheaf-visor@taulit.github.io')?.state === 1" 15
    shell_eval "$helpers" >/dev/null
    step "the module is registered" "sheafTest.modules().includes('notes')"

    shell_eval "sheafTest.ask('notiz Milch kaufen')" >/dev/null
    step "Visor offers to save the note" "sheafTest.results().find(g => g.id === 'command')?.titles[0] === 'Milch kaufen'"
    shell_eval "sheafTest.key(Clutter.KEY_Return, 300)" >/dev/null
    step "the notch confirms it" "sheafTest.chips().some(t => t.includes('Saved to Quick Notes'))" 5
    file="$SHEAF_NOTES_DIR/Quick Notes/Milch kaufen.md"
    for _ in $(seq 1 50); do [[ -f "$file" ]] && break; sleep 0.1; done
    if [[ -f "$file" ]]; then pass "the note is in the Quick Notes folder"; else fail "no $file"; fi
    shell_eval "sheafTest.key(Clutter.KEY_Escape)" >/dev/null

    # The app without a display draws its gallery on the headless platform; its startup
    # trace names the notes of the first frame with cards.
    trace="$HOME/startup.trace"
    env -u WAYLAND_DISPLAY -u DISPLAY SHEAF_STARTUP_TRACE="$trace" "$repo/target/debug/sheaf" \
        >"$HOME/app.log" 2>&1 &
    app=$!
    shown=false
    for _ in $(seq 1 300); do
        if grep -qxF 'card Quick Notes/Milch kaufen.md' "$trace" 2>/dev/null; then shown=true; break; fi
        kill -0 "$app" 2>/dev/null || break
        sleep 0.1
    done
    if $shown; then
        pass "the app's gallery shows it"
    else
        fail "the app's first frame has no Quick Notes/Milch kaufen.md: $(grep '^card' "$trace" 2>/dev/null | tr '\n' ';') $(tail -3 "$HOME/app.log")"
    fi
    kill -TERM "$app" 2>/dev/null || true
    wait "$app" 2>/dev/null || true

    provider='Main.overview.searchController._searchResults._providers.find(p => p.id === "io.github.taulit.Sheaf.desktop")'
    shell_eval 'Main.overview.show()' >/dev/null
    step "the overview opens" 'Main.overview.visible'
    shell_eval 'Main.overview.searchEntry.set_text("Milch")' >/dev/null
    step "the overview search finds it under Notes" \
        "(() => { const p = $provider; return !!p && p.appInfo.get_name() === 'Notes' && Object.values(p.display._resultDisplays).some(d => d.metaInfo.name === 'Milch kaufen'); })()" 30
    shell_eval 'Main.overview.hide()' >/dev/null
    ((failures == 0))
    exit
fi

visor=
while [[ $# -gt 0 ]]; do
    case "$1" in
        --visor) visor="$2"; shift 2 ;;
        *) echo "e2e.sh: unknown option $1" >&2; exit 2 ;;
    esac
done

cd "$repo"
echo "building (debug) ..." >&2
cargo build --workspace --quiet
summary=()
status=0

echo "5 Notiz anlegen (Visor, sheaf-searchd, the app, the overview)"
[[ -n "$visor" ]] || visor="$("$repo/scripts/build-visor-api-v1.sh")"
zip="$("$repo/scripts/pack-visor-module.sh")"
log="$repo/.shell-test/e2e-new-note.log"
mkdir -p "$repo/.shell-test"
if "$repo/scripts/visor-sandbox.sh" --visor "$visor" --module "$zip" --log "$log" -- \
    "$repo/scripts/e2e.sh" --inside 2>"${log%.log}.stderr"; then
    summary+=("pass  5 Notiz anlegen")
else
    summary+=("FAIL  5 Notiz anlegen (shell log $log, output ${log%.log}.stderr)")
    status=1
fi

echo "6 Externe Änderung (sed on an open checklist, through sheaf-searchd)"
if SHEAF_E2E_SEARCHD="$repo/target/debug/sheaf-searchd" \
    cargo test --quiet -p sheaf --test e2e -- --ignored 2>&1 | sed 's/^/  /'; then
    summary+=("pass  6 Externe Änderung")
else
    summary+=("FAIL  6 Externe Änderung")
    status=1
fi

echo
printf '%s\n' "${summary[@]}"
exit "$status"

#!/usr/bin/env bash
# Proves that the Visor module works inside a sandboxed headless GNOME Shell with the
# real Visor (tag api-v1), the module from its zip, a real sheaf-searchd and a
# stand-in for the Sheaf app (scripts/visor-sandbox.sh): the extension enables without
# errors and registers, the dock view lists the recent notes and creates notes, the search
# finds a note, `note hello` saves a quick note, the buttons reach the app, the "not
# installed" state shows when the desktop file goes, and enable/disable cycles are clean.
#
# Usage: scripts/visor-module-check.sh [--scheme dark|light] [--lang en|de] [--visor DIR] [--size WxH]
# Needs target/debug/sheaf-searchd (cargo build -p sheaf-searchd), gnome-shell 50,
# dbus-run-session, gdbus, jq, unzip, and a built Visor: --visor, or a local checkout of
# taulit/visor with the tag api-v1 (scripts/build-visor-api-v1.sh). Screenshots of the
# dock view go to docs/screenshots/visor-notes-<scheme>.png (English only), the shell
# log to .shell-test/visor-module-<scheme>-<lang>.log.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
uuid=sheaf-visor@taulit.github.io
visor_uuid=visor@taulit.github.io
desktop_id=io.github.taulit.Sheaf.desktop

if [[ "${1:-}" == --inside ]]; then
    source "$repo/scripts/lib/shell.sh"
    scheme="$2" lang="$3" shot="$4" log="$5"
    helpers="$(<"$repo/visor-module/tests/shell-helpers.js")"
    failures=0

    pass() { printf 'ok    %s\n' "$1"; }
    fail() { printf 'FAIL  %s\n' "$1"; failures=$((failures + 1)); }
    # check NAME JS EXPECTED: the JSON result of JS must equal EXPECTED.
    check() {
        local got
        got="$(shell_eval "$2" 2>&1)" || true
        if [[ "$got" == "$3" ]]; then pass "$1"; else fail "$1: expected $3, got $got"; fi
    }
    # check_wait NAME JS [seconds]: JS must become true.
    check_wait() {
        if shell_wait_for "$2" "${3:-10}" 2>/dev/null; then pass "$1"; else fail "$1: $(shell_eval "$2" 2>&1 | head -c 300)"; fi
    }
    # wait_file NAME TEXT: a note file with TEXT appears in the notes folder; prints its folder.
    wait_file() {
        local deadline=$((SECONDS + 10)) file
        while ((SECONDS < deadline)); do
            file="$(grep -rlF --include='*.md' -- "$2" "$SHEAF_NOTES_DIR" 2>/dev/null | head -1)" || true
            if [[ -n "$file" ]]; then pass "$1" >&2; basename "$(dirname "$file")"; return 0; fi
            sleep 0.2
        done
        fail "$1: no note with \"$2\" in $SHEAF_NOTES_DIR" >&2
        return 1
    }
    app_calls() { grep -c -- "$1" "$SHEAF_FAKE_APP_LOG" 2>/dev/null || true; }
    check_app() { # NAME PATTERN COUNT: the stand-in app got the call COUNT times
        local deadline=$((SECONDS + 10))
        while ((SECONDS < deadline)); do
            [[ "$(app_calls "$2")" == "$3" ]] && { pass "$1"; return; }
            sleep 0.2
        done
        fail "$1: expected $3 calls matching \"$2\", log: $(tr '\n' ';' <"$SHEAF_FAKE_APP_LOG" 2>/dev/null)"
    }
    create() {
        gdbus call --session --dest io.github.taulit.Sheaf.SearchProvider \
            --object-path /io/github/taulit/Sheaf/SearchProvider \
            --method io.github.taulit.Sheaf1.Create "$1" "${2:-}" >/dev/null
    }
    sync_index() {
        gdbus call --session --dest io.github.taulit.Sheaf.SearchProvider \
            --object-path /io/github/taulit/Sheaf/SearchProvider \
            --method io.github.taulit.Sheaf1.Sync >/dev/null
    }
    age() { # TITLE SECONDS: sets the modification time of the note file back
        local file
        file="$(grep -rl --include='*.md' -- "$1" "$SHEAF_NOTES_DIR" | head -1)"
        touch -d "@$(($(date +%s) - $2))" "$file"
    }
    # Ignores the noise of services the sandbox starts; what the shell or an extension says is checked.
    shell_errors() {
        grep -E 'JS ERROR|CRITICAL|already disposed|sheaf-visor|Visor: Notes' "$HOME/shell.log" |
            grep -v -E 'Extension .* already installed|search provider .* (missing DesktopId|ServiceUnknown)|Received error from D-Bus search provider' || true
    }

    # Strings of the UI in the language under test.
    if [[ "$lang" == de ]]; then
        s_new='Neue Notiz …' s_recent=Zuletzt s_open='In Notizen öffnen' s_quick=Schnellnotiz
        s_saved='In Schnellnotizen gespeichert' s_missing='Notizen ist nicht installiert' s_name=Notizen
        s_flathub='Auf Flathub ansehen'
    else
        s_new='New note …' s_recent=Recent s_open='Open in Notes' s_quick='Quick note'
        s_saved='Saved to Quick Notes' s_missing='Notes is not installed' s_name=Notes
        s_flathub='Get it on Flathub'
    fi
    # The sandbox has no translations of Sheaf itself, so the service names its folder in English.
    s_folder='Quick Notes'

    # --- the extensions ---
    gsettings set org.gnome.shell enabled-extensions "$SHEAF_ENABLED"
    check_wait "Visor is active" "Main.extensionManager.lookup('$visor_uuid')?.state === 1" 15
    check_wait "the module is active" "Main.extensionManager.lookup('$uuid')?.state === 1" 15
    check "the module has no error" "Main.extensionManager.lookup('$uuid').error" '""'
    shell_eval "$helpers" >/dev/null
    check_wait "the module is registered with Visor" "sheafTest.modules().includes('notes')"

    # --- notes to look at: titles, ages in seconds ---
    create $'Milch kaufen\nHafermilch und Butter nicht vergessen.' Schnellnotizen
    create $'Einkäufe für Samstag\n\n- [ ] Brot\n- [ ] Käse\n- [ ] Tomaten\n- [x] Eier'
    create $'Ausschreibung Rathaus\n\nFrist für das Angebot: 28. November. Unterlagen vom Bauamt abholen. Die Vergabestelle ist dienstags und donnerstags von neun bis zwölf Uhr erreichbar, Zimmer 214, zweiter Stock.' Arbeit
    create $'Quarterly planning\n\nAgenda: roadmap, hiring, budget review.' Arbeit
    create $'Pfannkuchen Rezept\n\n250 g Mehl, 3 Eier, 500 ml Milch, eine Prise Salz.'
    create $'Trip to Lisbon\n\nFlights on Friday, hotel near Alfama, pastéis de nata on day one.'
    create $'Reading list\n\n- [ ] The Pragmatic Programmer'
    create $'WLAN-Passwort Gäste\n\nRouter im Flur, Passwort steht auf dem Aufkleber unten.'
    sync_index
    age "Milch kaufen" 200
    age "Einkäufe für Samstag" 2400
    age "Ausschreibung Rathaus" 15000
    age "Quarterly planning" 100000
    age "Pfannkuchen Rezept" 300000
    age "Trip to Lisbon" 900000
    age "Reading list" 1500000
    age "WLAN-Passwort Gäste" 2500000
    sync_index

    # --- the dock view ---
    check "the dock icon opens the view" "sheafTest.showNotes()" true
    check_wait "the view shows six notes" "sheafTest.rows().length === 6"
    check "the notes come newest first" "sheafTest.rows().map(r => r[0])" \
        '["Milch kaufen","Einkäufe für Samstag","Ausschreibung Rathaus","Quarterly planning","Pfannkuchen Rezept","Trip to Lisbon"]'
    if [[ "$lang" == de ]]; then s_min='3 Min.'; else s_min='3 min'; fi
    check "the first note is three minutes old" "sheafTest.rows()[0][2]" "\"$s_min\""
    check "the quiet line of a note is its snippet" "sheafTest.rows()[0][1]" '"Hafermilch und Butter nicht vergessen."'
    check "the entry has its hint" "sheafTest.entry().hint_text" "\"$s_new\""
    check "the section is titled" "sheafTest.find('visor-section-title', sheafTest.view()).map(a => a.text)" "[\"$s_recent\"]"
    check "the view has its two actions" "sheafTest.viewActions()" 2
    check "the view has the module name as its title" "sheafTest.find('visor-view-title').map(a => a.text)" "[\"$s_name\"]"
    sleep 0.3
    [[ -z "$shot" ]] || shell_shot "$shot"

    # --- the entry creates notes ---
    sleep 1.2
    check "the entry has the key focus" "global.stage.get_key_focus() === sheafTest.entry().clutter_text" true
    shell_eval "sheafTest.type('Aus dem Feld')" >/dev/null
    shell_eval "sheafTest.key(Clutter.KEY_Return)" >/dev/null
    folder="$(wait_file "Enter in the entry creates a note" 'Aus dem Feld')" || true
    [[ "$folder" == "$s_folder" ]] && pass "it is in the Quick Notes folder" || fail "the folder is \"$folder\", not \"$s_folder\""
    check_wait "the list shows the new note first" "sheafTest.rows()[0]?.[0] === 'Aus dem Feld'"
    check "the entry is empty again" "sheafTest.entryText()" '""'
    sleep 1.2
    shell_eval "sheafTest.type('And open it')" >/dev/null
    shell_eval "sheafTest.ctrlEnter()" >/dev/null
    wait_file "Ctrl+Enter in the entry creates a note" 'And open it' >/dev/null || true
    check_app "Ctrl+Enter opens the note in the app" 'action open-note' 1
    check_wait "and closes the panel" "!sheafTest.panelOpen()"
    sleep 1.2

    # --- the buttons of the view ---
    shell_eval "sheafTest.showNotes()" >/dev/null
    check_wait "the view is back with its rows" "sheafTest.rows().length === 6"
    check "a click on a note opens it in the app" "sheafTest.click(sheafTest.find('visor-row', sheafTest.view())[1]).then(() => true)" true
    check_app "the app got a second open-note" 'action open-note' 2
    shell_eval "sheafTest.showNotes()" >/dev/null
    check "Quick note is a button" "sheafTest.buttons().includes('$s_quick')" true
    shell_eval "sheafTest.pressButton('$s_quick')" >/dev/null
    check_app "Quick note reaches the app" 'action quick-note' 1
    shell_eval "sheafTest.showNotes()" >/dev/null
    shell_eval "sheafTest.pressButton('$s_open')" >/dev/null
    check_app "Open in Notes activates the app" '^activate$' 1

    # --- search and command ---
    shell_eval "sheafTest.ask('Milch')" >/dev/null
    check_wait "the search finds the note" "sheafTest.results().find(g => g.id === 'source:notes')?.titles.includes('Milch kaufen')"
    check "Sheaf's GNOME search provider is hidden" "sheafTest.results().some(g => g.id === 'provider:$desktop_id')" false
     check "the group is titled by the module" "sheafTest.groupTitle('source:notes')" "\"$s_name\""
    shell_eval "sheafTest.key(Clutter.KEY_Escape)" >/dev/null
    shell_eval "sheafTest.ask('note hello from the check')" >/dev/null
    check_wait "the command shows the note" "sheafTest.results().find(g => g.id === 'command')?.titles[0] === 'hello from the check'"
    if grep -rqF --include='*.md' 'hello from the check' "$SHEAF_NOTES_DIR"; then fail "resolving saved a note"; else pass "resolving does not save anything"; fi
    shell_eval "sheafTest.key(Clutter.KEY_Return, 300)" >/dev/null
    folder="$(wait_file "the command saves the note" 'hello from the check')" || true
    [[ "$folder" == "$s_folder" ]] && pass "it is in the Quick Notes folder" || fail "the folder is \"$folder\", not \"$s_folder\""
    check_wait "the confirmation shows in the notch" "sheafTest.chips().some(t => t.includes('$s_saved'))" 5
    sleep 3.5
    shell_eval "sheafTest.ask('notiz')" >/dev/null
    check "the command without text offers the quick note" "sheafTest.results().find(g => g.id === 'command')?.titles[0]" "\"$s_quick\""
    shell_eval "sheafTest.key(Clutter.KEY_Return, 300)" >/dev/null
    check_app "the command without text opens the quick note window" 'action quick-note' 2

    # --- Sheaf is not installed ---
    rm "$SHEAF_SANDBOX_DATA/applications/$desktop_id"
    check_wait "the shell notices the removal" "Shell.AppSystem.get_default().lookup_app('$desktop_id') === null" 15
    shell_eval "sheafTest.showNotes()" >/dev/null
    check_wait "the view says Notes is not installed" "sheafTest.quiet().includes('$s_missing')"
    check "with a link to Flathub" "sheafTest.buttons().includes('$s_flathub')" true
    check "without the entry and the notes" "[sheafTest.entry().visible, sheafTest.rows().length]" '[false,0]'
    check "without header buttons" "sheafTest.viewActions()" 0
    shell_eval "sheafTest.ask('Milch')" >/dev/null
    check "the search stays quiet" "sheafTest.results().find(g => g.id === 'source:notes')?.titles ?? []" '[]'
    shell_eval "sheafTest.ask('note x')" >/dev/null
    check "the command points to Flathub" "sheafTest.results().find(g => g.id === 'command')?.titles[0]" "\"$s_missing\""
    shell_eval "sheafTest.key(Clutter.KEY_Escape)" >/dev/null
    cp "$repo/data/$desktop_id" "$SHEAF_SANDBOX_DATA/applications/"
    check_wait "Sheaf installed again" "Shell.AppSystem.get_default().lookup_app('$desktop_id') !== null" 15
    shell_eval "sheafTest.showNotes()" >/dev/null
    check_wait "the notes are back" "sheafTest.rows().length === 6"
    shell_eval "sheafTest.key(Clutter.KEY_Escape)" >/dev/null

    # --- enable and disable ---
    shell_eval "Main.extensionManager.disableExtension('$uuid')" >/dev/null
    check_wait "disabling the module removes it from Visor" "!sheafTest.modules().includes('notes')"
    check "no actor of the view is left" "sheafTest.find('sheaf-notes-view').length" 0
    shell_eval "Main.extensionManager.enableExtension('$uuid')" >/dev/null
    check_wait "enabling it registers it again" "sheafTest.modules().includes('notes')"
    shell_eval "Main.extensionManager.disableExtension('$visor_uuid')" >/dev/null
    check_wait "Visor goes" "Main.extensionManager.lookup('$visor_uuid').state !== 1"
    shell_eval "Main.extensionManager.enableExtension('$visor_uuid')" >/dev/null
    check_wait "Visor comes back and the module registers again" "Main.extensionManager.lookup('$visor_uuid').state === 1 && Main.extensionManager.lookup('$visor_uuid').stateObj.api !== null && sheafTest.modules().includes('notes')" 15
    shell_eval "(async () => { await sheafTest.wait(300); return true; })()" >/dev/null

    noise="$(shell_errors)"
    if [[ -z "$noise" ]]; then pass "the shell log has no error about the module"; else fail "the shell log:"$'\n'"$noise"; fi
    echo "shell log: $log"
    ((failures == 0))
    exit
fi

scheme=dark lang=en size=1280x800 visor=
while [[ $# -gt 0 ]]; do
    case "$1" in
        --scheme) scheme="$2"; shift 2 ;;
        --lang) lang="$2"; shift 2 ;;
        --size) size="$2"; shift 2 ;;
        --visor) visor="$2"; shift 2 ;;
        *) echo "visor-module-check.sh: unknown option $1" >&2; exit 2 ;;
    esac
done
[[ -n "$visor" ]] || visor="$("$repo/scripts/build-visor-api-v1.sh")"
zip="$("$repo/scripts/pack-visor-module.sh")"
shot=
[[ "$lang" == en ]] && shot="$repo/docs/screenshots/visor-notes-$scheme.png"
log="$repo/.shell-test/visor-module-$scheme-$lang.log"
if [[ "$lang" == de ]]; then export SHEAF_TEST_LOCALE=de_DE.UTF-8; fi

start=$SECONDS
status=0
"$repo/scripts/visor-sandbox.sh" --visor "$visor" --module "$zip" --scheme "$scheme" --size "$size" --log "$log" -- \
    "$repo/scripts/visor-module-check.sh" --inside "$scheme" "$lang" "$shot" "$log" || status=$?
if ((status == 0)); then
    echo "PASS ($scheme, $lang, $((SECONDS - start)) s)"
else
    echo "FAIL ($scheme, $lang): shell log $log" >&2
fi
exit "$status"

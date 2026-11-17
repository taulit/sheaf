#!/usr/bin/env bash
# Runs a command against a sandboxed headless GNOME Shell with Visor, the Visor module
# (from its zip), sheaf-searchd and a stand-in for the Sheaf app. Like
# scripts/shell-sandbox.sh, but without the real app, which is not needed here.
#
# The sandbox has its own HOME, XDG directories, runtime directory and D-Bus
# session, so nothing here can reach the settings, the notes or the shell of the
# session you are logged in to.
#
# Usage: scripts/visor-sandbox.sh --visor DIR --module ZIP [options] -- command [args...]
#   --visor DIR         a built Visor extension (the build/ directory of taulit/visor)
#   --module ZIP        the zip of scripts/pack-visor-module.sh
#   --size WxH          virtual monitor size (default 1280x800)
#   --scheme dark|light colour scheme (default dark)
#   --log FILE          copy the shell log to FILE when done
#
# Needs target/debug/sheaf-searchd (cargo build -p sheaf-searchd). Only the test helper is
# enabled when the command starts; SHEAF_ENABLED holds the list with Visor and the module for
# `gsettings set org.gnome.shell enabled-extensions`. Inside the command,
# scripts/lib/shell.sh offers shell_eval and shell_shot; SHEAF_NOTES_DIR is the notes
# folder of the sandbox, SHEAF_FAKE_APP_LOG the file the stand-in app writes its calls to,
# SHEAF_SANDBOX_DATA the data directory (for removing the desktop file of the app).
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bindir="$repo/target/debug"
size=1280x800 scheme=dark log= visor= module=

while [[ $# -gt 0 ]]; do
    case "$1" in
        --visor) visor="$(realpath "$2")"; shift 2 ;;
        --module) module="$(realpath "$2")"; shift 2 ;;
        --size) size="$2"; shift 2 ;;
        --scheme) scheme="$2"; shift 2 ;;
        --log) log="$2"; shift 2 ;;
        --) shift; break ;;
        *) echo "visor-sandbox.sh: unknown option $1" >&2; exit 2 ;;
    esac
done
[[ $# -gt 0 ]] || { echo "visor-sandbox.sh: missing command after --" >&2; exit 2; }
[[ -f "$visor/metadata.json" ]] || { echo "visor-sandbox.sh: --visor must be a built Visor directory" >&2; exit 2; }
[[ -f "$module" ]] || { echo "visor-sandbox.sh: --module must be the zip of the module" >&2; exit 2; }
[[ -x "$bindir/sheaf-searchd" ]] || { echo "visor-sandbox.sh: $bindir/sheaf-searchd missing, run 'cargo build -p sheaf-searchd'" >&2; exit 1; }

# Wayland socket paths are limited to 108 bytes, so the sandbox needs a short path.
sandbox="$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/sheaf-visor.XXXXXX")"
cleanup() {
    # Services activated on the sandbox bus outlive it for a while and keep
    # writing. Stop everything that carries the sandbox environment, release
    # the document portal's FUSE mount, then remove the directory.
    local environ pids
    for _ in $(seq 1 20); do
        pids=()
        for environ in $(grep -lsa "SHEAF_SANDBOX=$sandbox" /proc/[0-9]*/environ 2>/dev/null); do
            pids+=("$(basename "$(dirname "$environ")")")
        done
        ((${#pids[@]} > 0)) || break
        kill "${pids[@]}" 2>/dev/null || true
        sleep 0.15
    done
    for _ in 1 2 3 4 5 6; do
        fusermount3 -u -z "$sandbox/run/doc" 2>/dev/null || true
        rm -rf "$sandbox" 2>/dev/null || true
        [[ -e "$sandbox" ]] || break
        sleep 0.3
    done
}
trap cleanup EXIT
home="$sandbox/home"
data="$home/.local/share"
extensions="$data/gnome-shell/extensions"
module_uuid="$(unzip -p "$module" metadata.json | jq -r .uuid)"
mkdir -p "$home" "$sandbox/run" "$sandbox/notes" "$sandbox/bin" \
    "$data/applications" "$data/dbus-1/services" "$data/gnome-shell/search-providers" "$extensions"
chmod 700 "$sandbox/run"

cp -r "$repo/tests/shell-helper" "$extensions/sheaf-test-helper@taulit.github.io"
cp -r "$visor" "$extensions/visor@taulit.github.io"
unzip -q "$module" -d "$extensions/$module_uuid"
# GLib ignores a desktop file whose Exec program it cannot find. The real app is never started here.
printf '#!/bin/sh\nexit 0\n' >"$sandbox/bin/sheaf"
chmod +x "$sandbox/bin/sheaf"
cp "$repo/data/io.github.taulit.Sheaf.desktop" "$data/applications/"
cp "$repo/data/io.github.taulit.Sheaf-search-provider.ini" "$data/gnome-shell/search-providers/"
sed "s|@bindir@|$bindir|g" "$repo/data/io.github.taulit.Sheaf.SearchProvider.service.in" \
    >"$data/dbus-1/services/io.github.taulit.Sheaf.SearchProvider.service"
printf '[D-BUS Service]\nName=io.github.taulit.Sheaf\nExec=%s -m %s\n' \
    "$(command -v gjs)" "$repo/visor-module/tests/fake-sheaf-app.js" \
    >"$data/dbus-1/services/io.github.taulit.Sheaf.service"

status=0
env -i \
    PATH="$sandbox/bin:$bindir:$PATH" LANG="${LANG:-en_US.UTF-8}" LC_ALL="${SHEAF_TEST_LOCALE:-en_US.UTF-8}" TZ="${TZ:-Europe/Berlin}" \
    HOME="$home" XDG_RUNTIME_DIR="$sandbox/run" \
    XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$data" \
    XDG_DATA_DIRS="$data:/usr/local/share:/usr/share" XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state" \
    XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=GNOME \
    GIO_USE_VOLUME_MONITOR=unix GVFS_DISABLE_FUSE=1 \
    SHEAF_REPO="$repo" SHEAF_SANDBOX="$sandbox" SHEAF_NOTES_DIR="$sandbox/notes" \
    SHEAF_FAKE_APP_LOG="$sandbox/app.log" SHEAF_SANDBOX_DATA="$data" \
    SHEAF_SIZE="$size" SHEAF_SCHEME="$scheme" \
    SHEAF_ENABLED="['sheaf-test-helper@taulit.github.io', 'visor@taulit.github.io', '$module_uuid']" \
    dbus-run-session -- bash "$repo/scripts/lib/shell-session.sh" "$@" || status=$?

if [[ -n "$log" ]]; then
    mkdir -p "$(dirname "$log")"
    cp "$home/shell.log" "$log" 2>/dev/null || true
fi
exit "$status"

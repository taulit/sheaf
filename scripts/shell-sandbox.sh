#!/usr/bin/env bash
# Runs a command against a sandboxed headless GNOME Shell with Sheaf's desktop
# file, search provider and D-Bus service files installed.
#
# The sandbox has its own HOME, XDG directories, runtime directory and D-Bus
# session, so nothing here can reach the settings or the shell of the session
# you are logged in to.
#
# Usage: scripts/shell-sandbox.sh [options] -- command [args...]
#   --size WxH          virtual monitor size (default 1280x800)
#   --scheme dark|light colour scheme (default dark)
#   --log FILE          copy the shell log to FILE when done
#
# Needs target/debug/sheaf and target/debug/sheaf-searchd (cargo build --workspace);
# SHEAF_BINDIR=target/release takes the release build instead.
# Inside the command, scripts/lib/shell.sh offers shell_eval and shell_shot, and
# SHEAF_NOTES_DIR points at an empty notes folder of the sandbox.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bindir="${SHEAF_BINDIR:-$repo/target/debug}"
size=1280x800 scheme=dark log=

while [[ $# -gt 0 ]]; do
    case "$1" in
        --size) size="$2"; shift 2 ;;
        --scheme) scheme="$2"; shift 2 ;;
        --log) log="$2"; shift 2 ;;
        --) shift; break ;;
        *) echo "shell-sandbox.sh: unknown option $1" >&2; exit 2 ;;
    esac
done
[[ $# -gt 0 ]] || { echo "shell-sandbox.sh: missing command after --" >&2; exit 2; }
for bin in sheaf sheaf-searchd; do
    [[ -x "$bindir/$bin" ]] || { echo "shell-sandbox.sh: $bindir/$bin missing, run 'cargo build --workspace'" >&2; exit 1; }
done

# Wayland socket paths are limited to 108 bytes, so the sandbox needs a short path.
sandbox="$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/sheaf-shell.XXXXXX")"
cleanup() {
    # Services activated on the sandbox bus (portals, Evolution, the screencast
    # service, sheaf-searchd) outlive it for a while and keep writing. Stop
    # everything that carries the sandbox environment, release the document
    # portal's FUSE mount, then remove the directory.
    local environ pids
    # Services recreate their files (dconf, for one) while they shut down, so
    # wait until none is left before removing the directory.
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
mkdir -p "$home" "$sandbox/run" "$sandbox/notes" \
    "$data/applications" "$data/dbus-1/services" "$data/gnome-shell/search-providers" \
    "$data/gnome-shell/extensions"
chmod 700 "$sandbox/run"

cp -r "$repo/tests/shell-helper" "$data/gnome-shell/extensions/sheaf-test-helper@taulit.github.io"
cp "$repo/data/io.github.taulit.Sheaf.desktop" "$data/applications/"
cp "$repo/data/io.github.taulit.Sheaf-search-provider.ini" "$data/gnome-shell/search-providers/"
for service in "$repo"/data/*.service.in; do
    name="$(basename "${service%.in}")"
    sed "s|@bindir@|$bindir|g" "$service" >"$data/dbus-1/services/$name"
done
icon="$repo/data/icons/hicolor/scalable/apps/io.github.taulit.Sheaf.svg"
if [[ -f "$icon" ]]; then
    mkdir -p "$data/icons/hicolor/scalable/apps"
    cp "$icon" "$data/icons/hicolor/scalable/apps/"
fi

# GNOME Shell scans search providers only in XDG_DATA_DIRS, not in XDG_DATA_HOME,
# so the sandbox data dir is listed there as well.
status=0
env -i \
    PATH="$bindir:$PATH" LANG="${LANG:-en_US.UTF-8}" LC_ALL="${SHEAF_TEST_LOCALE:-en_US.UTF-8}" TZ="${TZ:-Europe/Berlin}" \
    HOME="$home" XDG_RUNTIME_DIR="$sandbox/run" \
    XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$data" \
    XDG_DATA_DIRS="$data:/usr/local/share:/usr/share" XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state" \
    XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=GNOME \
    GIO_USE_VOLUME_MONITOR=unix GVFS_DISABLE_FUSE=1 \
    SHEAF_REPO="$repo" SHEAF_SANDBOX="$sandbox" SHEAF_NOTES_DIR="$sandbox/notes" \
    SHEAF_SIZE="$size" SHEAF_SCHEME="$scheme" \
    dbus-run-session -- bash "$repo/scripts/lib/shell-session.sh" "$@" || status=$?

if [[ -n "$log" ]]; then
    mkdir -p "$(dirname "$log")"
    cp "$home/shell.log" "$log" 2>/dev/null || true
fi
exit "$status"

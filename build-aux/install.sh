#!/bin/sh
# Installs a built Sheaf into a prefix. The one place that knows the layout; the Flatpak
# manifest and the AUR packages call it.
#
#   build-aux/install.sh <prefix> [<destdir>]
#
# Files go to <destdir><prefix>/..., the D-Bus service files name <prefix>/bin. `sheaf` is
# taken from $SHEAF_BUILD_DIR (default: <target dir>/release, with <target dir> from
# $CARGO_TARGET_DIR or ./target), `sheaf-searchd` from $SHEAF_SERVICE_BUILD_DIR (default:
# <target dir>/service). Build first:
#   cargo build --release --locked -p sheaf
#   cargo build --profile service --locked -p sheaf-searchd
# The licence goes to share/licenses/$SHEAF_LICENSE_NAME (default: sheaf).
set -eu

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <prefix> [<destdir>]" >&2
    exit 2
fi
prefix="${1%/}"
destdir="${2:-}"
case "$prefix" in /*) ;; *) echo "$0: prefix must be absolute: $prefix" >&2; exit 2 ;; esac

root="$(cd "$(dirname "$0")/.." && pwd)"
app_id="io.github.taulit.Sheaf"
build_dir="${SHEAF_BUILD_DIR:-${CARGO_TARGET_DIR:-$root/target}/release}"
service_dir="${SHEAF_SERVICE_BUILD_DIR:-${CARGO_TARGET_DIR:-$root/target}/service}"

root_prefix="$destdir$prefix"
bindir="$root_prefix/bin"
datadir="$root_prefix/share"

for bin in "$build_dir/sheaf" "$service_dir/sheaf-searchd"; do
    [ -x "$bin" ] || { echo "$0: $bin not found; build first" >&2; exit 1; }
done

install -d "$bindir"
install -m 755 "$build_dir/sheaf" "$service_dir/sheaf-searchd" "$bindir/"

install -Dm644 "$root/data/$app_id.desktop" "$datadir/applications/$app_id.desktop"
install -Dm644 "$root/data/$app_id.metainfo.xml" "$datadir/metainfo/$app_id.metainfo.xml"
install -Dm644 "$root/data/icons/hicolor/scalable/apps/$app_id.svg" \
    "$datadir/icons/hicolor/scalable/apps/$app_id.svg"
install -Dm644 "$root/data/icons/hicolor/symbolic/apps/$app_id-symbolic.svg" \
    "$datadir/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"
install -Dm644 "$root/data/$app_id-search-provider.ini" \
    "$datadir/gnome-shell/search-providers/$app_id-search-provider.ini"

# The service files name the installed binaries; Flatpak rewrites Exec= when it exports them.
for name in "$app_id" "$app_id.SearchProvider"; do
    install -d "$datadir/dbus-1/services"
    sed "s|@bindir@|$prefix/bin|g" "$root/data/$name.service.in" > "$datadir/dbus-1/services/$name.service"
    chmod 644 "$datadir/dbus-1/services/$name.service"
done

# Translations (scripts/build-mo.sh writes <dir>/<lang>/LC_MESSAGES/sheaf.mo).
mo_stage="$(mktemp -d)"
trap 'rm -rf "$mo_stage"' EXIT
"$root/scripts/build-mo.sh" "$mo_stage"
for mo in "$mo_stage"/*/LC_MESSAGES/sheaf.mo; do
    lang="$(basename "$(dirname "$(dirname "$mo")")")"
    install -Dm644 "$mo" "$datadir/locale/$lang/LC_MESSAGES/sheaf.mo"
done

# Shell completions come from the binary itself.
completion() {
    shell="$1"
    dest="$2"
    install -d "$(dirname "$dest")"
    "$build_dir/sheaf" completions "$shell" > "$dest"
    chmod 644 "$dest"
}
completion bash "$datadir/bash-completion/completions/sheaf"
completion zsh "$datadir/zsh/site-functions/_sheaf"
completion fish "$datadir/fish/vendor_completions.d/sheaf.fish"

install -Dm644 "$root/LICENSE" "$datadir/licenses/${SHEAF_LICENSE_NAME:-sheaf}/LICENSE"

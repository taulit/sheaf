#!/bin/sh
# Installs stub binaries with build-aux/install.sh into a temporary prefix and checks the
# tree: files, service file paths, desktop and AppStream validity. No build needed.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
app_id="io.github.taulit.Sheaf"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/build"
cat > "$work/build/sheaf" <<'STUB'
#!/bin/sh
[ "$1" = completions ] && { echo "# stub completions for $2"; exit 0; }
exit 0
STUB
printf '#!/bin/sh\nexit 0\n' > "$work/build/sheaf-searchd"
chmod +x "$work/build/sheaf" "$work/build/sheaf-searchd"

prefix=/usr
dest="$work/dest"
SHEAF_BUILD_DIR="$work/build" "$root/build-aux/install.sh" "$prefix" "$dest"
tree="$dest$prefix"

fail=0
need() {
    if [ -e "$tree/$1" ]; then :; else echo "missing: $1" >&2; fail=1; fi
}
for f in \
    bin/sheaf bin/sheaf-searchd \
    share/applications/$app_id.desktop \
    share/metainfo/$app_id.metainfo.xml \
    share/icons/hicolor/scalable/apps/$app_id.svg \
    share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg \
    share/gnome-shell/search-providers/$app_id-search-provider.ini \
    share/dbus-1/services/$app_id.service \
    share/dbus-1/services/$app_id.SearchProvider.service \
    share/bash-completion/completions/sheaf \
    share/zsh/site-functions/_sheaf \
    share/fish/vendor_completions.d/sheaf.fish \
    share/licenses/sheaf/LICENSE; do
    need "$f"
done
for po in "$root"/po/*.po; do
    need "share/locale/$(basename "$po" .po)/LC_MESSAGES/sheaf.mo"
done

check_service() {
    file="$tree/share/dbus-1/services/$1.service"
    [ -e "$file" ] || return 0
    grep -qx "Name=$1" "$file" || { echo "$file: Name= is not $1" >&2; fail=1; }
    grep -q "^Exec=$prefix/bin/$2\( .*\)\?$" "$file" ||
        { echo "$file: Exec= does not name $prefix/bin/$2" >&2; fail=1; }
    ! grep -q '@' "$file" || { echo "$file: unreplaced @…@" >&2; fail=1; }
}
check_service "$app_id" sheaf
check_service "$app_id.SearchProvider" sheaf-searchd

# Nothing may land outside the prefix, and no stray files.
unexpected="$(cd "$dest" && find . -type f | sort | grep -v "^\./usr/\(bin\|share\)/" || true)"
[ -z "$unexpected" ] || { echo "files outside bin/ and share/: $unexpected" >&2; fail=1; }

desktop-file-validate "$tree/share/applications/$app_id.desktop" || fail=1
appstreamcli validate --no-net "$tree/share/metainfo/$app_id.metainfo.xml" || fail=1

if [ "$fail" -ne 0 ]; then
    echo "check-install: FAILED" >&2
    exit 1
fi
echo "check-install: ok ($(cd "$dest" && find . -type f | wc -l) files under $prefix)"

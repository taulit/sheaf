#!/bin/sh
# Regenerates po/sheaf.pot from the Rust sources and merges it into every po/*.po.
set -eu
cd "$(dirname "$0")/.."

find crates -name '*.rs' -not -path '*/tests/*' -not -path '*/benches/*' | LC_ALL=C sort > po/POTFILES
xgettext --language=Rust --from-code=UTF-8 \
    --keyword=tr --keyword=ntr:1,2 \
    --add-comments=TRANSLATORS \
    --package-name=sheaf --msgid-bugs-address=https://github.com/taulit/sheaf/issues \
    --copyright-holder='The Sheaf contributors' \
    --sort-by-file --no-wrap \
    --files-from=po/POTFILES --output=po/sheaf.pot

for po in po/*.po; do
    [ -e "$po" ] || continue
    msgmerge --quiet --update --backup=none --no-wrap --sort-by-file "$po" po/sheaf.pot
done

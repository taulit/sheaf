#!/bin/sh
# Regenerates visor-module/po/sheaf-visor.pot from the JavaScript sources and merges
# it into every visor-module/po/*.po. A new language is added with msginit.
set -eu
cd "$(dirname "$0")/../visor-module"

find extension.js lib -name '*.js' | LC_ALL=C sort > po/POTFILES
xgettext --language=JavaScript --from-code=UTF-8 \
    --keyword=t --keyword=gettext \
    --add-comments=TRANSLATORS \
    --package-name=sheaf-visor --msgid-bugs-address=https://github.com/taulit/sheaf/issues \
    --copyright-holder='The Sheaf contributors' \
    --sort-by-file --no-wrap \
    --files-from=po/POTFILES --output=po/sheaf-visor.pot

for po in po/*.po; do
    [ -e "$po" ] || continue
    msgmerge --quiet --update --backup=none --no-wrap --sort-by-file "$po" po/sheaf-visor.pot
done

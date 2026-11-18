#!/bin/sh
# Checks every translation catalog: complete (no fuzzy, no untranslated, valid formats),
# the same msgids as its template, and the same placeholders in msgid and msgstr.
# Usage: scripts/check-po.sh
set -eu
cd "$(dirname "$0")/.."

status=0
for dir in po visor-module/po; do
    pot="$(ls "$dir"/*.pot)"
    for po in "$dir"/*.po; do
        out="$(LC_ALL=C msgfmt --check --check-format --statistics -o /dev/null "$po" 2>&1)" || status=1
        case "$out" in
            *fuzzy*|*untranslated*) echo "$po: $out"; status=1 ;;
        esac
        python3 -I - "$pot" "$po" <<'PY' || status=1
import ast, re, sys

def entries(path):
    result, cur, field = {}, None, None
    for line in open(path, encoding="utf8"):
        line = line.rstrip("\n")
        if line.startswith("msgid "):
            cur, field = {"id": ast.literal_eval(line[6:]), "plural": None, "str": {}}, "id"
            result.setdefault(cur["id"], cur)
        elif line.startswith("msgid_plural "):
            cur["plural"], field = ast.literal_eval(line[13:]), "plural"
        elif line.startswith("msgstr"):
            m = re.match(r"msgstr(?:\[(\d+)\])? (.*)", line)
            field = int(m.group(1) or 0)
            cur["str"][field] = ast.literal_eval(m.group(2))
        elif line.startswith('"') and cur is not None:
            value = ast.literal_eval(line)
            if field in ("id", "plural"):
                cur[field] += value
            else:
                cur["str"][field] += value
    return result

pot_path, po_path = sys.argv[1:]
pot, po = entries(pot_path), entries(po_path)
problems = []
if set(pot) != set(po):
    problems += [f"missing msgid: {m!r}" for m in set(pot) - set(po)]
    problems += [f"unknown msgid: {m!r}" for m in set(po) - set(pot)]

token = re.compile(r"\{\w*\}|%\d*\$?[sd]")
for msgid, entry in po.items():
    if msgid not in pot or msgid == "" or msgid.startswith("%"):
        continue
    allowed = [sorted(token.findall(msgid))]
    if entry["plural"] is not None:
        allowed.append(sorted(token.findall(entry["plural"])))
    for index, text in entry["str"].items():
        if sorted(token.findall(text)) not in allowed:
            problems.append(f"placeholders differ in {msgid!r} [{index}]: {text!r}")
for problem in problems:
    print(f"{po_path}: {problem}")
sys.exit(1 if problems else 0)
PY
    done
done
exit "$status"

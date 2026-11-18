#!/usr/bin/env bash
# Copies the demo notes into $SHEAF_NOTES_DIR and dates them so the gallery shows
# "Today", "Yesterday" and "Previous 7 Days" groups.
set -euo pipefail
src="$(cd "$(dirname "$0")/../demo-notes" && pwd)"
dst="${SHEAF_NOTES_DIR:?}"
cp -r "$src/." "$dst/"
stamp() { touch -d "$2" "$dst/$1"; }
stamp "Personal/Groceries.md"                 "today 08:05"
stamp "Work/Launch plan.md"                   "today 09:12"
stamp "Work/Ideas for the notes app.md"       "today 11:02"
stamp "Quick Notes/Call the dentist.md"       "today 10:15"
stamp "Work/Meeting notes – design review.md" "today 09:40"
stamp "Recipes/Weeknight ramen.md"            "today 07:30"
stamp "Personal/Trip to Lisbon.md"            "yesterday 18:20"
stamp "Recipes/Banana bread.md"               "yesterday 16:05"
stamp "Personal/Workout.md"                   "yesterday 07:45"
stamp "Work/Rust reading list.md"             "3 days ago 21:10"
stamp "Personal/Book ideas.md"                "4 days ago 22:30"
stamp "Quick Notes/Wi-Fi at the studio.md"    "4 days ago 13:00"
stamp "Personal/Gift ideas.md"                "5 days ago 19:15"
stamp "Recipes/Pancakes.md"                   "6 days ago 09:00"

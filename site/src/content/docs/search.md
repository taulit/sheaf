---
title: Search syntax
description: The Sheaf search language - typo-tolerant words, quoted phrases, #tags and the folder, has, open, modified and pinned filters.
---

Start typing anywhere in the gallery, or press **Ctrl+F**. Results update with every key.
With 10,000 notes a search takes about 6 ms (95th percentile, measured on the release
build).

## Words

- Every word must match. A word also matches as the start of a longer word (`launc` finds
  "launch") and with a small typo (`lauch` finds it too).
- Accents and umlauts are folded: `cafe` finds "Café", `muller` finds "Müller".
- `"quoted text"` must appear exactly as written.
- Matches in the title count more than matches in the text.

## Filters

| Filter | Finds notes that |
|---|---|
| `#tag` | have the tag, or one nested below it (`#work` matches `#work/launch`) |
| `folder:name` | are in a folder of that name; `folder:/Work` only at the top level |
| `has:checklist` | contain a checklist (also `has:image`, `has:table`, `has:attachment`) |
| `open:` | have unchecked checklist items |
| `modified:today` | changed today (also `modified:week`, `modified:month`) |
| `pinned:` | are pinned |

Filters and words combine: `ramen #recipes has:checklist`.

German keywords work as well, whatever the interface language is (`ordner:`, `hat:` and so
on).

## Save a search

Save any search as a **smart folder** to get it as a chip above the gallery.

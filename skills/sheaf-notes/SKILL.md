---
name: sheaf-notes
description: Read, search, create and update the user's notes with the `sheaf` command line. Use when the user asks to remember something, take or find a note, add to a list, check off a checklist item, or look up what they wrote down, and the notes live in Sheaf (plain Markdown files).
---

# Sheaf notes

Sheaf stores notes as Markdown files. Always go through the `sheaf` CLI, never through
the files, so ids, file names, trash and search index stay consistent.

## How to call it

- Add `--json` to every command except `show`. The result is one JSON object with
  `"schema": 1`; errors are a JSON object on stderr (`error.code`, `error.message`).
- Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 ambiguous. After 4, choose from
  `error.candidates` and repeat the command with the `id`.
- A note is addressed by `id` (26 characters), exact title, or unique title prefix.
  Keep the `id` from the first result and reuse it.
- Text comes from the argument or from stdin. The first line of a note is its title.
- Nothing prompts. `rm` only moves to the trash (`sheaf trash`, `sheaf restore`).

## Commands

| Goal | Command |
|---|---|
| New note | `printf '%s' "$TEXT" \| sheaf new --json [--folder F] [--tag T] [--pin]` |
| Add to a note | `sheaf append <NOTE> "text" --json` · `sheaf prepend …` (below the title) |
| Find | `sheaf search 'words #tag folder:Work has:checklist open:' --limit 10 --json` |
| Browse | `sheaf list [--folder F] [--tag T] [--pinned] [--modified-since YYYY-MM-DD] [--sort modified\|created\|title] --json` |
| Read | `sheaf show <NOTE> --json` (metadata and `body`), or without `--json` for the Markdown |
| Tasks | `sheaf tasks [--open] [--note <NOTE>] --json`; `sheaf check <NOTE> <NUMBER\|TEXT> --json`, `sheaf uncheck …` |
| Organise | `sheaf mv <NOTE> <FOLDER>`, `sheaf pin <NOTE>`, `sheaf unpin <NOTE>`, `sheaf rm <NOTE>`, `sheaf tags`, `sheaf folders` |
| Describe the CLI | `sheaf help --json` |

Query filters: `#tag`, `folder:Name`, `has:checklist|image|table|attachment`, `open:`
(unchecked items), `pinned:`, `modified:today|week|month`.

## Working rules

1. To answer from the notes, `search` first, then `show` the best hit by `id`. Quote
   what the note says; do not invent content.
2. To add something, prefer `append` to an existing note over creating a duplicate.
   Search for the title before `new`.
3. To tick an item, run `tasks --note <NOTE> --json`, then `check <NOTE> <number>`
   with that number. Text matching only works if exactly one item contains the text.
4. Report the note `title` and `id` you changed. Do not delete anything unless the user
   asked; then use `rm`, which is undoable.
5. Do not edit or delete files in the notes folder, `.sheaf/` or `.trash/`.
6. Do not set `--dir` unless the user names a folder (`sheaf path` prints the one in use).

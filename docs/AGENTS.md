# Reading and writing Sheaf notes from an AI agent

Sheaf keeps notes as Markdown files. Use the `sheaf` command to read and change them:
it keeps ids, file names, the trash and the index consistent. Output formats are listed
in [cli-schema/](cli-schema/README.md); `sheaf help --json` describes every command.

## Rules

- Always pass `--json` (except for `show`, see below). You get one JSON object on
  stdout, with `"schema": 1` first. Branch on `error.code` and the exit status, never on
  `message`.
- Address a note by `id` (26 characters, stable), by exact title, or by a unique title
  prefix. Ids survive renames and moves; titles do not. Take `id` from `new`, `search`
  or `list` and keep using it.
- Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 ambiguous. On 4,
  `error.candidates` lists `{id, title, folder}`; pick one and retry with its id.
- Commands never prompt and never open a window. Text comes from the argument or from
  stdin (`-` forces stdin). The first line of a note is its title.
- `rm` moves to the trash; `sheaf trash` lists it and `sheaf restore` undoes it.

## Examples

```sh
# Create a note from stdin; the id is in .note.id
printf 'Standup 2026-11-16\n- [ ] Ship the CLI\n\n#team' | sheaf new --json --folder Work

# Append to it (a title prefix is enough, an id is safest)
sheaf append "Standup 2026-11-16" "- [ ] Write the docs" --json

# Search with the app's query language: words, #tag, folder:, has:, open:, pinned:
sheaf search 'release notes #team has:checklist' --limit 5 --json

# Recently changed notes in a folder
sheaf list --folder Work --modified-since 2026-11-01 --sort modified --json

# Open checklist items of all notes, or of one
sheaf tasks --open --json
sheaf tasks --note 01JD3K8Z6Q4X7V9B2M5N8P0R3T --json

# Check an item by its number (from `tasks`) or by a unique piece of its text
sheaf check 01JD3K8Z6Q4X7V9B2M5N8P0R3T 2 --json
sheaf uncheck "Standup 2026-11-16" "ship the cli" --json

# Read a note: JSON has metadata and body; the default prints the Markdown only
sheaf show 01JD3K8Z6Q4X7V9B2M5N8P0R3T --json
sheaf show 01JD3K8Z6Q4X7V9B2M5N8P0R3T            # text for a prompt, no frontmatter

# Organise
sheaf mv "Standup 2026-11-16" Work/2026 --json
sheaf pin "Standup 2026-11-16" --json
sheaf tags --json && sheaf folders --json
```

Each command that changes a note returns it under `note` (`id`, `title`, `folder`,
`path`, `modified`, `tags`, `tasks_open`, …). `search` hits add `score`, `snippet` and
byte ranges `snippet_highlights` and `title_highlights`.

## Do not

- Do not edit or delete files in the notes folder, in `.sheaf/` (settings, templates)
  or in `.trash/` yourself. Use `sheaf`; direct writes can lose ids, break the index
  and clash with the sync tool. Reading a file by the `path` of a note is fine.
- Do not parse the human output (no `--json`) or error `message` texts.
- Do not guess ids. An id is 26 characters; anything else is a title.
- Do not run `sheaf` without a subcommand; it exits with status 2.
- Do not pass `--dir` unless the user named a folder; `sheaf path` shows the one in use.

# CLI output schemas

`sheaf <command> --json` prints exactly one JSON document on stdout. A failing command
prints one JSON document on stderr instead ([error.schema.json](error.schema.json)).
Each document is an object whose first key is `"schema": 1`. The files here are
[JSON Schema](https://json-schema.org/) draft 2020-12; they refer to each other with
relative `$ref`s, so keep them in one folder. `sheaf help --json` names the schema file
of every command (`output_schema`).

| Command | Schema |
|---|---|
| `new`, `append`, `prepend`, `edit`, `check`, `uncheck`, `mv`, `rm`, `restore`, `pin`, `unpin`, `open` | [note.schema.json](note.schema.json) (also defines the note object used everywhere) |
| `show` | [show.schema.json](show.schema.json) |
| `list` | [note-list.schema.json](note-list.schema.json) |
| `search` | [search.schema.json](search.schema.json) |
| `tasks` | [tasks.schema.json](tasks.schema.json) |
| `tags` | [tags.schema.json](tags.schema.json) |
| `folders` | [folders.schema.json](folders.schema.json) |
| `trash` | [trash.schema.json](trash.schema.json) |
| `path` | [path.schema.json](path.schema.json) |
| `quick`, `reindex` | [done.schema.json](done.schema.json) |
| `help` | [help.schema.json](help.schema.json) |
| any failing command | [error.schema.json](error.schema.json) |

`completions` prints a shell script, never JSON. `show` without `--json` (or
`--format json`) prints the note text, not a document.

## Versioning

- Additive changes keep `"schema": 1`: a new key in an object, a new command, a new
  error `code`. Readers must ignore keys they do not know.
- Anything else raises the number: removing or renaming a key, changing a type or the
  meaning of a value, changing an exit code. The old schema files stay available under
  the old number.
- The tests validate every document the CLI prints against these files, and reject keys
  the files do not describe, so the two cannot drift apart unnoticed.

Strings are not translated in JSON. `message` in an error is for people; branch on
`code` and the exit status.

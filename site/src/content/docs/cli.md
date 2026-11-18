---
title: Command line and JSON output
description: The sheaf command line - create, search, list and change notes from scripts or AI agents, with stable JSON output.
---

`sheaf` with a command works without a window. It keeps ids, file names, the trash and the
search index consistent, so it is the safe way for scripts to change notes.

```sh
sheaf new "Milk"                                  # create a note, prints its id
sheaf search 'milk #shopping has:checklist' --json
sheaf list --folder Work --modified-since 2026-11-01
sheaf tasks --open                                # open checklist items of all notes
sheaf append "Milk" "- [ ] oat drink"
sheaf help                                        # every command
```

- `--json` prints one JSON document with `"schema": 1`. The schemas are in
  [docs/cli-schema](https://github.com/taulit/sheaf/tree/main/docs/cli-schema).
- Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 ambiguous.
- A note is addressed by its id, its exact title or a unique title prefix.
- `sheaf rm` moves to Recently Deleted, `sheaf restore` brings a note back.

## For AI agents

[docs/AGENTS.md](https://github.com/taulit/sheaf/blob/main/docs/AGENTS.md) explains how an
agent should read and write notes, and
[skills/sheaf-notes](https://github.com/taulit/sheaf/tree/main/skills/sheaf-notes) is a
ready skill folder.

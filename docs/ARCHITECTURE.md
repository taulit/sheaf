# Architecture

Sheaf keeps notes as plain Markdown files and builds everything else around them. This
file records the decisions that span more than one crate. The plan it implements is in
[design/plan/03-sheaf.md](design/plan/03-sheaf.md).

## Parts

| Part | Kind | Role |
|---|---|---|
| `sheaf-core` | library | Notes folder (read, write, rename, trash, watch), index, search language. No D-Bus, no UI |
| `sheaf-searchd` | binary | D-Bus service without a window. The only process that writes the index |
| `sheaf` | binary | The GPUI app, and the CLI when called with a subcommand |
| `visor-module/` | GNOME extension | "Visor: Notes", talks to `sheaf-searchd` over D-Bus |

## Notes on disk

- The notes folder holds one `.md` file per note. Subfolders are the app's folders.
- Hidden entries (`.sheaf/`, `.trash/`, `.git/`, `.stfolder`, …) and `_attachments/` are
  never treated as notes or folders.
- **Title** is the first non-empty line of the text, without heading markers. Sheaf does
  not add a `# ` to what the user typed.
- **File name** follows the title: `/`, `\`, control characters and a leading `.` are
  replaced, the name is trimmed to 120 bytes at a character boundary, an empty title
  becomes the localised "Untitled". On a collision ` 2`, ` 3`, … is appended.
- **Frontmatter** is written only when there is something to store. Sheaf edits single
  keys in place and leaves every other byte of the file alone, so a file Sheaf did not
  change is byte-identical after a read/write cycle.
- **Writes are atomic**: write to a temporary file in the same folder, `fsync`, rename.

### Note ids

A note written by Sheaf has `id: <ULID>` in its frontmatter. A file Sheaf has never
written has none, and Sheaf does not modify files just to stamp them (a synced-in vault
of 5,000 files would otherwise be rewritten and re-synced). Such a note gets a
*provisional* id derived from its relative path (`p` + 25 lower-case base32 characters).
The first time Sheaf writes that note, it adds a real ULID. From then on the id is stable
across renames and moves. Callers that hold an id across a write must use the id the
write returns. See `NoteId` in `crates/sheaf-core/src/model.rs`.

## Where state lives

| What | Where |
|---|---|
| Notes folder location | `$XDG_CONFIG_HOME/sheaf/config.toml` (`notes_dir`), overridden by `SHEAF_NOTES_DIR` or `--dir` |
| Default notes folder | `$XDG_DOCUMENTS_DIR/Notes` (localised: `Notizen`), chosen on first run |
| Per-folder state | `<notes>/.sheaf/config.toml`: smart folders, sort order, per-note view state |
| Templates | `<notes>/.sheaf/templates/*.md` |
| Trash | `<notes>/.trash/`, each file with `deleted:` and `from:` in its frontmatter |
| Index | `$XDG_CACHE_HOME/sheaf/index/<hash of the notes folder path>/`, safe to delete |

## Index: one writer, many readers

- `sheaf-searchd` owns the tantivy `IndexWriter`. It watches the notes folder, re-indexes
  changed files and commits within 300 ms. On start it reconciles the index with the
  folder (path + mtime + size), so changes made while it was not running are picked up.
- The app opens the same index read-only and reloads on commit. The gallery is drawn
  from stored index fields, not from the files.
- The CLI reads and writes note files through `sheaf-core` directly. For `search` it
  asks `sheaf-searchd` over D-Bus. If there is no session bus or the service cannot be
  started, it builds a temporary in-memory index of the folder instead: slower, but
  always correct, and what the snapshot tests use.

## D-Bus

| Bus name | Owner | Interfaces |
|---|---|---|
| `io.github.taulit.Sheaf.SearchProvider` | `sheaf-searchd` | `org.gnome.Shell.SearchProvider2`, `io.github.taulit.Sheaf1` at `/io/github/taulit/Sheaf/SearchProvider` |
| `io.github.taulit.Sheaf` | the app | `org.freedesktop.Application` at `/io/github/taulit/Sheaf` |

`sheaf-searchd` never opens windows. To show a note it calls
`org.freedesktop.Application.ActivateAction` on the app's name, which starts the app
through D-Bus activation if it is not running. Actions:

| Action | Parameters | Effect |
|---|---|---|
| `open-note` | `(s id, s query)` | Open the note; jump to the first match of `query` if not empty |
| `search` | `(s query)` | Show the gallery with this search |
| `quick-note` | none | Open the Quick Note window |

A second `sheaf` launch without a subcommand calls `Activate` on the running instance
and exits.

## Text

Every user-visible string goes through gettext (`sheaf_core::i18n`, text domain
`sheaf`). English is the source language. JSON output of the CLI is not translated.

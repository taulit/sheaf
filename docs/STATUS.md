# Status

Other sessions read only this file. Updated at the end of every phase.

| Phase | State | Branch / PR |
|---|---|---|
| S0 Setup | merged | [#1](https://github.com/taulit/sheaf/pull/1) |
| S1 Core, searchd, CLI | merged | [#2](https://github.com/taulit/sheaf/pull/2), [#3](https://github.com/taulit/sheaf/pull/3) |
| S2 App | gate green, in review | `phase/s2-app` |
| S3 Features | not started | |
| S4 Release | not started | |

## What works (S1)

- `sheaf-core`: notes folder (lossless frontmatter, atomic writes, trash, watcher), index
  and search language. Search p95 5.7 ms for 10,000 notes (budget 30 ms).
- `sheaf-searchd`: GNOME Shell search provider and `io.github.taulit.Sheaf1` on D-Bus,
  about 11 MB idle (budget 15 MB). The D-Bus contract is `crates/sheaf-searchd/src/api.rs`.
- `sheaf` CLI with JSON output, schemas in `docs/cli-schema/`, agent guide `docs/AGENTS.md`.
- `scripts/shell-search-check.sh` proves the GNOME overview finds notes, in a sandboxed
  headless shell (`docs/testing.md`).

## What works (S2)

- `sheaf` without a subcommand starts the GPUI app: single instance on
  `io.github.taulit.Sheaf`, theme from the Settings portal, window size remembered.
- Gallery: cards with miniatures of the content, date groups, search with filter chips and
  highlighted hits, folder picker, list view, multi-select, context menu, undo toast.
- Note view: saves 600 ms after the last change and at once on leave, close, quit and
  SIGTERM; the file follows the title; changes from other programs reload or ask; a save
  that fails shows a banner and falls back to `$XDG_STATE_HOME/sheaf/recovered/`.
- Command palette (Ctrl+K), keyboard shortcuts (Ctrl+?), toasts; German translation.
- The editor shows Markdown source until `gpui-markdown-editor` renders it (tag
  `v0.0.1-api`). Screenshots: `docs/screenshots/app-*.png` (`scripts/screenshot.sh`).

## Tags

None yet.

## Depends on

| What | State on 2026-11-16 | Effect |
|---|---|---|
| `taulit/gpui-markdown-editor` tag `v0.0.1-api` or newer | `v0.0.1-api` exists (checked 2026-11-16, 22:25): API complete, plain-text editing behind it | S2 builds against it; gpui-pre =0.3.12, gpui-component =0.10.0, MSRV 1.95 |
| `taulit/visor` tag `api-v1` | no tag yet (checked 2026-11-16, 22:25) | "Visor: Notes" (plan 03 §10) is open; built in S3 if the tag exists then, else in session 4 |

## Open

- `flatpak-builder` and `git-cliff` are not installed on the build machine; needed in S4.
- Not yet verified, see `docs/verified.md` "Plan changes" 10: Flathub acceptance of
  `--filesystem=xdg-documents` (S4), passing an activation token to a window opened later
  by the running GPUI app (S2), reading the Settings portal inside the sandbox (S4).
- `rust-version` goes from 1.91 to 1.92 when `krilla` is added for PDF export (S3).
- S2 CI needs the repository secret `EDITOR_DEPLOY_KEY` (read-only deploy key on
  `taulit/gpui-markdown-editor`); not created yet.
- S2 review leftovers, all minor, to fix at the start of S3: in service mode the gallery
  shows the old title, or a just-trashed note, until the service's `Changed` signal
  arrives (`app/library.rs`); a second SIGTERM/SIGINT is ignored while the final save
  runs, and SIGHUP now quits under `nohup` (`app/mod.rs`); pin, move and duplicate from
  the gallery drop errors without a toast.
- Wanted from the editor (API): read and set the scroll position, a getter for the scroll
  target, a slot above the first block (date line), asynchronous completions.
- The S2 app was checked through tests and offscreen screenshots only, not by hand in a
  real desktop session.

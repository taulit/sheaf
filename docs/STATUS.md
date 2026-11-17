# Status

Other sessions read only this file. Updated at the end of every phase.

| Phase | State | Branch / PR |
|---|---|---|
| S0 Setup | merged | [#1](https://github.com/taulit/sheaf/pull/1) |
| S1 Core, searchd, CLI | merged | [#2](https://github.com/taulit/sheaf/pull/2), [#3](https://github.com/taulit/sheaf/pull/3) |
| S2 App | gate green, in review | `phase/s2-app` |
| S3 Features | merged locally, gate green, not pushed (GitHub login lost) | `phase/s3-features` |
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
- The editor renders and edits the note in place (`gpui-markdown-editor` `v0.0.3`);
  Ctrl+K stays the command palette inside it. Screenshots: `docs/screenshots/app-*.png`
  (`scripts/screenshot.sh`).

## What works (S3)

- Quick Note window (`sheaf --quick`, D-Bus, Ctrl+Shift+N, the GlobalShortcuts portal):
  380×300, folder menu, Open and Done, continues the last quick note within the
  configured minutes.
- Recently Deleted: trashed notes with days left, read-only view, Restore, Delete
  Permanently, Empty Trash. Smart folders (saved searches) and templates (`+ ▾`,
  `{{date}}`, `{{time}}`, `{{title}}`).
- Export (share popover): Markdown copy without frontmatter, PDF through the FileChooser
  portal, Print through the Print portal with a PDF for the chosen paper; when printing
  fails the toast offers the PDF in a viewer. `sheaf-export` sets the PDF itself
  (krilla, HarfRust shaping, Inter embedded as subsets, system mono and emoji
  fallbacks, page numbers, outline, links).
- Attachments: dropped, pasted (files or images) or chosen files are copied to
  `<folder>/_attachments/<id>/` and linked at the caret.
- Backlinks: "N notes link here" under the text, folded; trashed notes are left out.
- Settings window (Ctrl+,): notes folder (the whole app follows), default order (folders
  without their own), Quick Note folder and resume time (`.sheaf/config.toml`), editor
  font and line width (note view, quick note, trashed note), the global shortcut with
  its GNOME fallback, the keyboard shortcuts per context.
- "Visor: Notes" in `visor-module/` against Visor `api-v1`, tested in a sandboxed shell
  (`scripts/test-visor-module.sh`).
- Gate on 2026-11-17 after the merges: fmt and clippy clean, 411 tests passed (4
  ignored: screenshots, run by `scripts/screenshot.sh`), `cargo deny check` ok, Visor
  module 22 of 22.

## Tags

None yet.

## Depends on

| What | State on 2026-11-16 | Effect |
|---|---|---|
| `taulit/gpui-markdown-editor` tag `v0.0.3` | used since 2026-11-17 (newest tag then): WYSIWYG editing, `highlight`, `scroll_to`, `set_style`, format bar, slash menu with host items, completions, image resolver, `MetaChanged` | gpui-pre =0.3.12, gpui-component =0.10.0, MSRV 1.95 |
| `taulit/visor` tag `api-v1` | exists (checked 2026-11-17) | "Visor: Notes" is built in `visor-module/` against it (S3) |

## Open

- `flatpak-builder` and `git-cliff` are not installed on the build machine; needed in S4.
- Not yet verified, see `docs/verified.md` "Plan changes" 10: Flathub acceptance of
  `--filesystem=xdg-documents` (S4), passing an activation token to a window opened later
  by the running GPUI app (S2), reading the Settings portal inside the sandbox (S4).
- `rust-version` is 1.95 (the editor's MSRV), above the 1.92 that `krilla` needs.
- S3 is not pushed: `gh` lost its login on 2026-11-17, `git push` fails. `phase/s3-features`
  is ahead of origin; no S3 PR yet, and no `bug-review` over the S3 diff yet.
- Not built from plan 03 §9: `/vorlage` in the slash menu (the editor's `SlashItem`s are
  fixed when the editor is created, so they cannot follow the templates), the Print
  portal is not probed beforehand (Print shows always; the fallback comes after a
  failure). Attachments of trashed notes stay in `_attachments/`, and images do not
  show in the read-only trashed note (relative paths point beside the original).
- PDF limits: no italic face (slanted Inter), checked items are grey but not struck
  through, `#tags` are not coloured, right-to-left text is set left to right, no
  hyphenation.
- CI is red and ignored for now (user's decision, 2026-11-17): it cannot fetch the
  private `taulit/gpui-markdown-editor`. The gate runs locally before every PR.
- In service mode folder and tag counts follow the service's index, not the app's own
  writes (`app/library/overlay.rs`).
- Wanted from the editor (API, not in `v0.0.3`): read and set the scroll position (a
  reload from disk scrolls to the top), a getter for the scroll target, a slot above the
  first block (the date line does not scroll with the text), asynchronous completions,
  slash items that can change after creation (for `/vorlage`).
  Also letter spacing (title −0.02em) and tabular figures for table numbers.
- The S2 and S3 app was checked through tests and offscreen screenshots only, not by hand
  in a real desktop session; no real portal dialog (FileChooser, Print, GlobalShortcuts)
  was opened.

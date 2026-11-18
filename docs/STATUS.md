# Status

Other sessions read only this file. Updated at the end of every phase.

| Phase | State | Branch / PR |
|---|---|---|
| S0 Setup | merged | [#1](https://github.com/taulit/sheaf/pull/1) |
| S1 Core, searchd, CLI | merged | [#2](https://github.com/taulit/sheaf/pull/2), [#3](https://github.com/taulit/sheaf/pull/3) |
| S2 App | merged | [#4](https://github.com/taulit/sheaf/pull/4) |
| S3 Features | merged | [#5](https://github.com/taulit/sheaf/pull/5) |
| S4 Release | gate green, in review | [#6](https://github.com/taulit/sheaf/pull/6) |

## What works (S1)

- `sheaf-core`: notes folder (lossless frontmatter, atomic writes, trash, watcher), index
  and search language. Search p95 5.7 ms for 10,000 notes (budget 30 ms).
- `sheaf-searchd`: GNOME Shell search provider and `io.github.taulit.Sheaf1` on D-Bus,
  12.1 MB idle with 1,000 notes, 13.4 MB with 10,000 (budget 15 MB). The D-Bus contract is `crates/sheaf-searchd/src/api.rs`.
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
- The editor renders and edits the note in place (`gpui-markdown-editor` `v0.1.0`);
  Ctrl+K stays the command palette inside it, the editor's link prompt is on Ctrl+Shift+K.
  Format bar, slash menu (`/vorlage` and `/template` insert a template), find bar
  (Ctrl+F), folding, tables, inline images. Dropped and pasted files and images become
  attachments, also in the Quick Note (which has no format bar). Screenshots: `docs/screenshots/app-*.png`
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
  `<folder>/_attachments/<id>/` and linked at the caret; they go to the trash with
  their note, except files another note still links to.
- Backlinks: "N notes link here" under the text, folded; trashed notes are left out.
- Settings window (Ctrl+,): notes folder (the whole app follows), default order (folders
  without their own), Quick Note folder and resume time (`.sheaf/config.toml`), editor
  font and line width (note view, quick note, trashed note), the global shortcut with
  its GNOME fallback, the keyboard shortcuts per context.
- "Visor: Notes" in `visor-module/` against Visor `api-v1`, tested in a sandboxed shell
  (`scripts/test-visor-module.sh`).
- Gate on 2026-11-17 after the review fixes: fmt and clippy clean, 436 tests passed (4
  ignored: screenshots, run by `scripts/screenshot.sh`), `cargo deny check` ok, Visor
  module 23 of 23.

## What works (S4 review fixes, 2026-11-18)

- Release workflow: the tag reaches shell only through `env`, the version must be strict
  semver, only the draft-release job may write.
- `sheaf-searchd` rebuilds an index removed while it was idle, and restarts for its malloc
  tunables at most once (`SHEAF_SEARCHD_RESTARTED`).
- The default notes folder is recorded in `config.toml` on first run (an existing
  `Notes`/`Notizen` is taken), so a language change keeps it.
- Sync conflict copies have a badge on card and row and a hint in the note view.
- Card menu: Export (Markdown…, PDF…; several notes into a chosen folder).
- Folder popover: tags with counts and a pin each (pinned tag chips have Unpin), Rename… and
  Delete… on folders (notes to the trash with Undo; view, Quick Notes folder and smart-folder
  searches follow a rename), New Folder / New Smart Folder stay in view.
- Smart folders get one of ten GNOME symbols, chosen under the name field (`icon`).
- Two-finger swipe to the right on the touchpad goes back from a note.
- With Visor and its Notes module active, the GlobalShortcuts portal is left alone and the
  settings say so.
- CLI messages and core errors are translated; clap's help texts stay English.
- Gate on 2026-11-18: fmt and clippy clean, 476 tests passed (5 ignored: screenshots, run
  by `scripts/screenshot.sh`), `cargo deny check` ok, Visor module 23 of 23, budgets all
  pass, `scripts/e2e.sh` 5 and 6 pass, `check-install` and `check-release 0.1.0` ok, German
  complete (251 messages).

## Budgets

Measured on 2026-11-18 with `scripts/budgets.sh` (release build, `--locked`, commit `dcb7512`;
`sheaf-searchd` with its profile `service`), Intel Core i5-11400F, 12 threads, 31 GB, Manjaro, Linux
6.18. How each number is taken: `docs/testing.md`, "Budgets".

| Budget (05 §1) | Target | Measured | |
|---|---|---|---|
| Cold start to the gallery, 1,000 notes | < 250 ms | 167 ms to the frame with every miniature, 115 ms to the first cards (median of 8 warm starts; first start 172 ms) | pass |
| Memory, 1,000 notes, gallery open | < 120 MB RSS | 67.0 MB RSS, 61.2 MB PSS | pass |
| Search p95, 10,000 notes | < 30 ms | 6.2 ms | pass |
| Gallery scrolling, 10,000 notes | 60 fps | CPU frame p50 3.4 ms, p95 10.7 ms, max 21.1 ms, 1 of 600 over 16.7 ms | pass |
| `sheaf-searchd` idle | < 15 MB | 12.3 MB RSS with 1,000 notes, 13.4 MB with 10,000 (PSS 7.6 and 8.7 MB) | pass |

- The start and memory are measured on the headless platform: no Wayland connection, GPU device,
  shaders or presenting, and no GPU memory. Scrolling is CPU time per frame, not frames on screen.
- `sheaf-searchd` was at 16.6 / 18.1 MB. Now: built for size with the Cargo profile `service`
  (16.3 -> 14.0 / 17.8 -> 15.7 MB) and restarted once at start with glibc malloc tunables
  (one arena, fixed trim threshold; -> 12.1 / 13.4 MB), besides letting go of the index 5 s after
  the last request (`docs/ARCHITECTURE.md`, "Index"). What stays is about 10 MB of mapped files,
  most of it its own code (5.4 MB) and the locale archive (2.3 MB): the kernel maps page-cache
  folios whole, so a few touched pages bring in much of a file. Search through D-Bus on 10,000
  notes went from p95 8.6-10.0 ms to 12.0 ms (gdbus round trips).
- Only the search budget runs in CI; the others are a local script (GPUI release build, timing noise on
  shared runners, and CI cannot fetch the editor at the moment).

## End-to-end scenarios

`scripts/e2e.sh`: 5 "Notiz anlegen" (Visor, `sheaf-searchd`, the app's gallery, the overview search) and
6 "Externe Änderung" (`sed` on an open checklist, through `sheaf-searchd`) pass. What needs a person at a
desktop is a checklist in `docs/testing.md`. `scripts/visor-module-check.sh` (main en/dark, de/light,
absent, old) and `scripts/shell-search-check.sh` (dark, light) pass on the current code.

## Tags

None yet.

## Depends on

| What | State on 2026-11-16 | Effect |
|---|---|---|
| `taulit/gpui-markdown-editor` tag `v0.1.0` | used since 2026-11-18 (newest tag): WYSIWYG editing, `highlight`, `scroll_to`, `set_style`, format bar, slash menu with host items, find bar, folding, tables, images, completions, `MetaChanged`. API as in `v0.0.3`; empty paragraphs are no longer written | gpui-pre =0.3.12, gpui-component =0.10.0, MSRV 1.95 |
| `taulit/visor` tag `api-v1` | exists (checked 2026-11-17) | "Visor: Notes" is built in `visor-module/` against it (S3) |

## Open

- `flatpak-builder` and `git-cliff` are not installed on the build machine; needed in S4.
- Not yet verified, see `docs/verified.md` "Plan changes" 10: Flathub acceptance of
  `--filesystem=xdg-documents` (S4), passing an activation token to a window opened later
  by the running GPUI app (S2), reading the Settings portal inside the sandbox (S4).
- `rust-version` is 1.95 (the editor's MSRV), above the 1.92 that `krilla` needs.
- S3 is pushed. Its `bug-review` found 12 issues, all fixed on 2026-11-17
  (`a8c46c4..21290e9`); a narrow re-review of the fixes and the S3 PR are next.
- After the S4 review fixes: the eight further languages of the plan README and a narrow
  re-check of the fix commits (`8aee14a..HEAD`) are next. Folder rename gives notes without a
  Sheaf id new provisional ids (path-derived), so their per-note view state (column widths,
  folds) does not follow. Visor detection is read from source (`org.gnome.Shell.Extensions`),
  not tried with a real Visor. The swipe is tested with synthetic events only; its distance
  (140 px) and the pause that ends a gesture (180 ms) need a try on a real touchpad. Nothing in
  the note text scrolls sideways today; if the editor gets that, the swipe has to leave it alone.
- Not built from plan 03 §9: the Print portal is not probed beforehand (Print shows
  always; the fallback comes after a failure). Images do not show in the read-only
  trashed note (gpui-component's `TextView`, since the editor has no read-only mode).
- PDF limits: no italic face (slanted Inter), checked items are grey but not struck
  through, `#tags` are not coloured, right-to-left text is set left to right, no
  hyphenation.
- CI is red and ignored for now (user's decision, 2026-11-17): it cannot fetch the
  private `taulit/gpui-markdown-editor`. The gate runs locally before every PR.
- In service mode folder and tag counts follow the service's index, not the app's own
  writes (`app/library/overlay.rs`).
- Wanted from the editor (API, still not in `v0.1.0`): read and set the scroll position (a
  silent reload from disk scrolls to the top), slots above the first and below the last
  block (the date line and "Linked from" do not scroll with the text, and the format bar
  floats above an open "Linked from"), asynchronous completions (`[[`/`#` use a cached
  lookup), slash items that can change after creation (`/vorlage` opens Sheaf's own
  menu), a read-only mode (trashed notes use `TextView`), a setter for `max_width` (line
  width is padding), a drop hook (Sheaf lays a drop target over the editor while
  something is dragged; the editor would link dropped files where they lie), a hook for
  pasted files (Sheaf takes Ctrl+V; `on_image` is synchronous and drops the file name),
  letter spacing (title −0.02em). Tables have tabular figures since `v0.0.4`.
- GPUI 0.3.12: file drops do not switch the input mode to the mouse, so right after
  typing a drop lands nowhere until the mouse has moved over the window.
- The S2 and S3 app was checked through tests and offscreen screenshots only, not by hand
  in a real desktop session; no real portal dialog (FileChooser, Print, GlobalShortcuts)
  was opened.
- Every settings change rewrites `config.toml` and `.sheaf/config.toml` through
  `toml::Table`: values and unknown keys survive, hand-written comments and key order do
  not (`sheaf-core/src/config.rs`, `write_table`). A candidate for `toml_edit` in S4.
- The S3 app was checked through tests, offscreen screenshots and a sandboxed headless
  GNOME Shell (Visor module), not by hand in a real desktop session.

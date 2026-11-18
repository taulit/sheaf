# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), the versions follow
[Semantic Versioning](https://semver.org/).

The release workflow takes the notes of a release from the section of its version. On release
day the date after the version is set to the day of the tag.

## [0.1.0] - 2026-11-17

First release.

### Added

- Notes as plain Markdown files in a folder you choose (by default Documents/Notes, kept
  when the language changes), with folders as subfolders. Files that
  Sheaf did not change stay byte for byte as they were. Changes made by other programs or by
  sync tools are picked up while the app runs.
- Gallery of cards with a miniature of each note, grouped by date, and a list view. Folders,
  tags, pinned notes, multi-select and a context menu with export. Folders are renamed and
  deleted from the folder popover, where tags are pinned to the chip row too. Sync tools'
  conflict copies are marked.
- Back from a note with a two-finger swipe to the right on the touchpad.
- Editing in place with checklists, tables, images, links between notes, a format bar, a
  slash menu (`/template` inserts a template), find in the note (Ctrl+F) and folding of
  sections (editor `gpui-markdown-editor` 0.1.0). The link prompt is Ctrl+Shift+K. Notes save a moment after the last change and when you leave them.
- Search that finds a note in milliseconds, forgives typos, knows German umlauts and
  understands `#tag`, `folder:`, `has:`, `open:`, `modified:` and `pinned:`. Hits are
  highlighted in the cards and in the note.
- Smart folders (saved searches with a symbol) and templates with `{{date}}`, `{{time}}` and
  `{{title}}`.
- Quick Note window, started with `sheaf --quick`, from the app menu or with Ctrl+Shift+N
  through the GlobalShortcuts portal; with Visor on, Visor supplies the shortcut.
- Recently Deleted with restore and delete forever. Attachments, backlinks ("N notes link
  here") and a command palette (Ctrl+K).
- Export as Markdown or PDF, and printing through the Print portal.
- Notes in the GNOME overview, through a search provider.
- Settings window: notes folder, sort order, Quick Note, editor font and line width, global
  shortcut and the keyboard shortcuts.
- Command line for scripts and agents: `sheaf new`, `search`, `list`, `show`, `append`,
  `tasks` and more, with `--json` output, fixed exit codes and shell completions for bash,
  zsh and fish. See `docs/AGENTS.md`.
- "Visor: Notes", a GNOME Shell extension that shows recent notes in the Visor dock and saves
  quick notes with the command `note`.
- German translation.
- Packages: Flatpak manifest, AUR packages `sheaf` and `sheaf-git`.

[0.1.0]: https://github.com/taulit/sheaf/releases/tag/v0.1.0

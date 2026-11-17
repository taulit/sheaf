# Handoff (session 3, Sheaf)

Written 2026-11-17 for whoever continues without the session's context. Read
`docs/design/plan/README.md` (sessions, working rules, pull requests) and
`docs/STATUS.md` first.

## Done

- **S0** and **S1** are on `main` (PRs #1, #2 via #3): workspace, CI, `sheaf-core`
  (storage, search), `sheaf-searchd`, CLI, GNOME search check, German translation.
- **S2** on `phase/s2-app` (pushed, based on `main`): app foundation, gallery, note view,
  overlays (toast, command palette, shortcuts), merged and integrated. 317 tests passed,
  clippy and fmt clean, before the review fixes started. Screenshots: `docs/screenshots/app-*.png`,
  regenerate with `scripts/screenshot.sh`.

## Running

- **Fixing the S2 bug-review findings** (child agent "S2 gate: fix review findings",
  works in this checkout on `phase/s2-app`, commits one fix per finding). If it stopped
  half-way: `git status` shows its uncommitted work; finish or commit it as WIP.

## Open review findings (S2 bug review; all must be fixed, each with a test)

1. Blocking: closing the window or quitting right after typing loses the edit. The final
   save is only queued on a foreground worker; the write must be guaranteed before exit.
   Needs a test on the real headless platform, not `TestAppContext`.
2. Blocking: a failed save is silent; the editor is marked saved before the write.
   Needs a banner, retry, and a recovery file as last resort.
3. Blocking: note deleted elsewhere, user leaves without "Save as New Note": text lost,
   depending on event order.
4. Major: a failed trash leaves the note open but no longer saving.
5. Major: an empty new note is purged on leave without checking the file's content.
6. Major (not demonstrated): gallery cover images decoded at full size, never released.
7. Minor: deleting a note that changed on disk trashes the conflict copy.
8. Minor (suspected): underflow restoring the scroll anchor, `gallery/mod.rs` near `row_of`.
9. Visual: header title is not centred on the window.

The reviewer did not read `gallery/render.rs`, `note/header.rs`, `note/date.rs`,
`overlays/**`, `theme.rs`, `chrome.rs`, `assets.rs`, the CLI changes. Check `git log` for
`fix(` commits to see which findings are already done.

## Next

1. After the fixes: a fresh review agent re-checks the fixes and reads the files listed
   above (skill `bug-review`), until no blocking finding is left.
2. S2 gate, run once: `cargo fmt --check`, `cargo clippy --workspace --all-targets
   --all-features -- -D warnings`, `cargo test --workspace --all-features`, `cargo deny
   check`, `scripts/screenshot.sh`, look at every screenshot, try the app by hand
   (`SHEAF_NOTES_DIR=<tmp> cargo run -p sheaf`).
3. Open the S2 PR **against `main`** (skill `file-pr`), screenshots dark and light
   embedded. Update `docs/STATUS.md`.
4. S3 on `phase/s3-features`, branching from `phase/s2-app`: quick note, trash view,
   smart folders, templates, backlinks, attachments, export (PDF with `krilla`, see
   `docs/verified.md` §2), settings; the Visor module only if `taulit/visor` has the tag
   `api-v1` (`gh api repos/taulit/visor/git/refs/tags/api-v1`).
5. S4: budgets, translations, reviews, Flatpak, AUR, metainfo, README.

## Waiting for the user

- CI for S2 needs the secret `EDITOR_DEPLOY_KEY` (read-only deploy key on the private
  `taulit/gpui-markdown-editor`). Not created; needs the user's go-ahead.
- PR screenshots are embedded
  from the repository instead.

## For the editor session

Sheaf needs from `gpui-markdown-editor` (API, not only implementation): reading and
setting the scroll position (or a `set_markdown` that keeps it), a getter for the scroll
target, a slot for content above the first block (date line), and asynchronous
completions.

## Working notes

- Sub-agents: Opus for implementation, Sonnet for mechanical work, Fable for reviews and
  gates. Worktree agents should use `CARGO_TARGET_DIR=~/development/sheaf/target`.
- Phase PRs target `main` directly (a stacked base sent #2 into the wrong branch).
- The editor is pinned at `v0.0.1-api` (raw Markdown text). Check for a newer tag with
  `gh api repos/taulit/gpui-markdown-editor/git/refs/tags` and bump it in `Cargo.toml`.

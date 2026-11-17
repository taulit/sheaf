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

All three S3 work packages are merged, integrated and pushed on `phase/s3-features`.
A child agent ("S3 gate: fix review findings") fixes the findings of the S3 bug review
in this checkout, one commit and push per finding; check `git log` for `fix(` commits
after `fd2978d`.

## Open findings of the S3 bug review

1. Blocking: the gallery can overwrite `.sheaf/config.toml` with defaults (load and
   reload share one task field in `gallery/mod.rs`; whole-struct saves from stale copies).
   Fix at the root: every config writer changes only its own keys, read-modify-write
   under the Library's config lock, never before the config was loaded.
2. Major: a Quick Note window whose note cannot be created cannot be closed (`quick.rs`).
3. Major: trash/purge of a note takes attachments other notes still link to
   (`sheaf-core/src/trash.rs`).
4. Major: PDF export reads any path an image names (`/dev/zero`, huge files)
   (`sheaf-export/src/layout.rs`, `load_image`).
5. Minor: PDF export overflows the stack on ~3000 nested quotes or lists.
6. Minor: settings that could not be read are overwritten with defaults.
7. Minor: a failed write of folder settings is only logged.
8. Minor: overlay entries stay when a sync is not confirmed (`library/service.rs`).
9. Minor: attaching a large file loads it into memory under the note's write lock.
10. Minor: the Shell extension can drop typed text when Create fails.
11. Minor: "Open as PDF" after a failed print leaves the PDF in the shared temp dir.
12. Minor: Enter confirms "Delete Permanently" and "Empty Trash".

After the fixes: a narrow re-review of the fix commits, the gate, then the S3 PR
against `main` (needs #4 merged first, or the PR shows S2's commits too).

## Second and third S2 review: no blocking finding left

All ten findings of the second review are fixed (`7c9b81e..e0b424c`) and confirmed by a
third, narrow review. Its three minor findings and one more leftover are listed in
`docs/STATUS.md` under "Open" and are the first task of S3.

## Review findings of the first S2 bug review: all fixed (commits `9e47287..90700b4`)

Close or quit right after typing, silent failed save (banner, retry, recovery folder
`$XDG_STATE_HOME/sheaf/recovered/`), note deleted elsewhere, failed trash, discard of an
empty new note, cover image cache, delete during a conflict, scroll anchor underflow,
header title centring. After the fixes: fmt, clippy, `cargo deny check` clean, all
workspace tests pass (`tests/shutdown.rs` runs on the real headless platform).

Confirmed by the second review, except the cover cache (finding 3 above).

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

- **CI is red until `taulit/gpui-markdown-editor` is public.** The organisation has
  deploy keys disabled, so the user chose (2026-11-17) to make the editor repository
  public. The workflow no longer uses ssh or a secret. Once it is public:
  `gh run rerun --failed` on the PRs. No key and no secret exist.
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
- The editor is pinned at `v0.0.3` (WYSIWYG). Check for a newer tag with
  `gh api repos/taulit/gpui-markdown-editor/git/refs/tags` and bump it in `Cargo.toml`.

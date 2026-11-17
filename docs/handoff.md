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

S2 is in review: https://github.com/taulit/sheaf/pull/4 (against `main`; CI fails until the
secret `EDITOR_DEPLOY_KEY` exists). S3 runs on `phase/s3-features` (branched from S2):
the editor is at `v0.0.3` (rendered Markdown) and the S2 leftovers are fixed there.

Three child agents work in their own worktrees, branched from `phase/s3-features`, and
push their branches (`git branch -r | grep t3/`) after every commit:

- WP-B: quick note, trash view, smart folders, templates.
- WP-C: export (Markdown, PDF with `krilla`, print), settings window, backlinks, attachments.
- WP-D: the Visor module in `visor-module/` (Visor has the tag `api-v1`).

Next: merge the three branches into `phase/s3-features` (shared files `app/window.rs`,
`app/actions.rs`, `app/library.rs`, `tests/screenshots.rs`, `po/` will conflict; keep
both sides, regenerate `po/` and the screenshots), run the gate, one `bug-review` over
`git diff phase/s2-app...phase/s3-features`, fix, then the S3 PR against `main` (after
#4 is merged; until then against `phase/s2-app` is wrong: wait or rebase).

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
- The editor is pinned at `v0.0.3` (WYSIWYG). Check for a newer tag with
  `gh api repos/taulit/gpui-markdown-editor/git/refs/tags` and bump it in `Cargo.toml`.

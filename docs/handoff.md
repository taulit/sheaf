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

Nothing. All three S3 work packages are merged into `phase/s3-features` **locally**:
the Visor module (`061a76a`), quick note, trash, smart folders and templates
(`e584815`), export, settings, backlinks and attachments (`9c840d5`), then integrated
and polished (`1d60611..`). Gate on 2026-11-17: fmt and clippy clean, all workspace
tests pass, `cargo deny check` ok, `scripts/test-visor-module.sh` 22 of 22, screenshots
regenerated and looked at, a sample PDF checked. See `docs/STATUS.md` "What works (S3)"
and "Open".

The pushes are pending because of the lost GitHub login (below): `phase/s3-features`
is ahead of origin. S2 is still in review: https://github.com/taulit/sheaf/pull/4.

Next: after the login is back, push `phase/s3-features`; one `bug-review` over `git diff
phase/s2-app...phase/s3-features`, fix; then the S3 PR against `main` once #4 is merged.

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

- **`gh` is logged out on this machine** (token invalid since 2026-11-17 evening), and
  `git push` over https fails with it. Run `gh auth login -h github.com`, then push
  `phase/s3-features` and the `t3/*` branches that are ahead of origin.
- The user approved the CI deploy key (2026-11-17). Not created yet because of the login:
  `ssh-keygen -t ed25519 -N "" -f <tmp>/key`, `gh repo deploy-key add <tmp>/key.pub --repo
  taulit/gpui-markdown-editor --title "sheaf CI (read-only)"` (no `--allow-write`),
  `gh secret set EDITOR_DEPLOY_KEY --repo taulit/sheaf < <tmp>/key`, delete the files,
  `gh run rerun --failed` on PR #4.
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

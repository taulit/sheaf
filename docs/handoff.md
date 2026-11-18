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

S2 is merged (#4). S3 is in review: https://github.com/taulit/sheaf/pull/5 (against
`main`; its checks are red because CI cannot fetch the editor, see below).
S4 on `phase/s4-release` (branched from `phase/s3-features`, everything pushed): budgets
measured and all met (`scripts/budgets.sh`, table in `docs/STATUS.md`), end-to-end
scenarios automated (`scripts/e2e.sh`), packaging merged (`build-aux/`, metainfo,
release workflow, README, `docs/release-checklist.md`), editor at `v0.1.0`.

Three read-only review agents run in this checkout (2026-11-18): "S4 gate: bug review"
(diff `phase/s3-features...phase/s4-release`), "S4 gate: spec review" (whole app against
plan 03/04/05) and "S4 gate: simplicity review" (whole code base).

## Next

1. Collect the three reports; fix blocking and major bug findings and the spec findings
   that are plan requirements for 1.0; apply only the low-risk simplifications; one
   narrow re-check of the fixes.
2. Gate: fmt, clippy, tests, deny, Visor tests, `scripts/budgets.sh`, `scripts/e2e.sh`,
   `scripts/screenshot.sh` (copy the untracked `docs/screenshots/gnome-search-light.png`
   aside first: the shell check scripts overwrite it).
3. S4 PR against `main` after #5 is merged (`main` gets squash merges: sync with
   `git merge -s ours origin/main` only after checking that main's tree equals
   `phase/s3-features`). Fill `docs/release-checklist.md` as the final report.
4. Release blockers that need the user: both repositories are private (Flathub, AUR
   users and CI cannot fetch them; metainfo screenshot URLs 404), `flatpak-builder` is not
   installed (Flatpak build untested), the manual desktop checklist in `docs/testing.md`.

## Waiting for the user

- Nothing about CI. The user decided on 2026-11-17: `taulit/gpui-markdown-editor` stays
  private for now and the red PR checks are ignored (CI cannot fetch the editor). The
  gate is therefore run locally before every PR: `cargo fmt --check`, clippy with
  `-D warnings`, `cargo test --workspace --all-features`, `cargo deny check`,
  `scripts/test-visor-module.sh`, `scripts/screenshot.sh`; state the results in the PR.
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
- The editor is pinned at `v0.1.0`. Check for a newer tag with
  `gh api repos/taulit/gpui-markdown-editor/git/refs/tags` and bump it in `Cargo.toml`.

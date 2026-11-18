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

Nothing. S2 is merged (#4). S3 (https://github.com/taulit/sheaf/pull/5) and S4 (see
`gh pr list`; it contains S3) are in review against `main`. PR checks are red because CI
cannot fetch the private editor; the gate was run locally on 2026-11-18 at the head of
`phase/s4-release`: fmt, clippy, `cargo deny` clean, 487 tests passed, Visor tests 23 of
23, `scripts/check-po.sh`, `check-install.sh`, `check-release.sh 0.1.0`, all budgets
(`scripts/budgets.sh`) and both end-to-end scenarios (`scripts/e2e.sh`) pass.

## Next

Only release work that needs the user (details in `docs/release-checklist.md`):
make the repositories public (Flathub, AUR users and CI cannot fetch them; metainfo
screenshot URLs 404), build the Flatpak once (`flatpak-builder` is not installed here),
run the manual desktop checklist in `docs/testing.md`, set the release date in metainfo
and changelog, tag, AUR, Flathub, extension upload. The nine translations other than
German were not read by native speakers. What the editor still lacks for Sheaf is listed
in `docs/STATUS.md`; bump the editor tag when a newer one exists.

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

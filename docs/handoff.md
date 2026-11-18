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

Nothing. S2 is merged (#4). S3 is in review as a PR against `main` (see `gh pr list`).
Its three review rounds left no blocking or major finding; what is still open is in
`docs/STATUS.md` under "Open".

## Next

1. S4 on `phase/s4-release`, branching from `phase/s3-features` (or from `main` once the
   S3 PR is merged; `main` gets squash merges, so sync a branch that was built on the
   unsquashed commits with `git merge -s ours origin/main` only after checking that the
   trees match, as done for S3): budgets (start time, memory, search), translations
   check, a final review, Flatpak manifest, AUR `PKGBUILD`, metainfo, README, release
   notes. `flatpak-builder` and `git-cliff` are not installed on this machine.
2. Check for newer editor tags (`gh api repos/taulit/gpui-markdown-editor/git/refs/tags`)
   and bump from `v0.0.3`; the editor still lacks format bar, slash menu, completions,
   image loading, `MetaChanged` and the APIs listed in `docs/STATUS.md`.

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
- The editor is pinned at `v0.0.3` (WYSIWYG). Check for a newer tag with
  `gh api repos/taulit/gpui-markdown-editor/git/refs/tags` and bump it in `Cargo.toml`.

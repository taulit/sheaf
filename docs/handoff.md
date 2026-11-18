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
`main`, mergeable; its checks are red because CI cannot fetch the editor, see below).
S4 runs on `phase/s4-release` (branched from `phase/s3-features`) with two child agents:

- WP-A (this checkout, commits and pushes per step): `scripts/budgets.sh` for the budgets
  of plan 05 §1 (cold start < 250 ms and < 120 MB RSS with 1,000 notes, search p95
  < 30 ms with 10,000, gallery frame time), fixes to meet them, `scripts/e2e.sh` for the
  end-to-end scenarios of 05 §3, test gaps of 05 §2.
- WP-B (own worktree, pushes its `t3/*` branch): metainfo, `build-aux/install.sh`,
  Flatpak manifest, AUR PKGBUILDs, release workflow, `CHANGELOG.md`, README,
  `docs/release-checklist.md`.

## Next

1. Merge WP-B into `phase/s4-release`, run the gate (fmt, clippy, tests, deny, Visor
   tests, `scripts/budgets.sh`, `scripts/e2e.sh`, screenshots).
2. Reviews, once each, fresh agents (plan 05 §4): `bug-review` over
   `git diff phase/s3-features...phase/s4-release`, plus `simplicity-review` and
   `spec-review` over the whole app against `docs/design/plan/03-sheaf.md`; fix together.
3. S4 PR against `main` (after #5 is merged; `main` gets squash merges, so first sync
   with `git merge -s ours origin/main` after checking that the trees match).
4. Known release blockers: Flathub and CI cannot fetch the private
   `taulit/gpui-markdown-editor`; `flatpak-builder` and `git-cliff` are not installed
   here; newer editor tags may exist (`gh api repos/taulit/gpui-markdown-editor/git/refs/tags`).

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

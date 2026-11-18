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

The three S4 reviews are done (2026-11-18): bug review 0 blocking (1 major: shell
injection through the tag name in `release.yml`; 3 minor), spec review 16 deviations not
known before, simplicity review 18 proposals (13 low risk). A child agent ("S4 gate: fix
review findings") works through them in this checkout, one commit and push per item:
A bug findings 1-3, B plan gaps 4-12 (default notes folder fixed at first start, conflict
badge, Export in the card menu, tag pinning in the app, swipe back, skip the shortcut
portal when Visor is installed, smart-folder icon, folder rename/delete, CLI messages
through gettext), C low-risk simplifications 13-17. Decided not to do: the `≡` menu,
saving the window position, TypeScript for the Visor module, CLI writes through the
service, removing the overlay of own writes or the key-wise config writer.

## Next

1. After the fixes: translations for the eight further languages of the plan README
   („Übersetzungen“: es, fr, pt_BR, it, pl, ru, zh_CN, ja) into `po/` and
   `visor-module/po/` (Sonnet), then one narrow re-check of the fix commits.
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

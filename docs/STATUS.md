# Status

Other sessions read only this file. Updated at the end of every phase.

| Phase | State | Branch / PR |
|---|---|---|
| S0 Setup | in review | `phase/s0-setup`, [#1](https://github.com/taulit/sheaf/pull/1) |
| S1 Core, searchd, CLI | gate green, in review | `phase/s1-core` |
| S2 App | not started | |
| S3 Features | not started | |
| S4 Release | not started | |

## What works (S1)

- `sheaf-core`: notes folder (lossless frontmatter, atomic writes, trash, watcher), index
  and search language. Search p95 5.7 ms for 10,000 notes (budget 30 ms).
- `sheaf-searchd`: GNOME Shell search provider and `io.github.taulit.Sheaf1` on D-Bus,
  about 11 MB idle (budget 15 MB). The D-Bus contract is `crates/sheaf-searchd/src/api.rs`.
- `sheaf` CLI with JSON output, schemas in `docs/cli-schema/`, agent guide `docs/AGENTS.md`.
- `scripts/shell-search-check.sh` proves the GNOME overview finds notes, in a sandboxed
  headless shell (`docs/testing.md`).

## Tags

None yet.

## Depends on

| What | State on 2026-11-16 | Effect |
|---|---|---|
| `taulit/gpui-markdown-editor` tag `v0.0.1-api` or newer | repository does not exist yet | S2 editor view waits; S1 does not need it |
| `taulit/visor` tag `api-v1` | repository does not exist yet | "Visor: Notes" (plan 03 §10) is open; built in S3 if the tag exists then, else in session 4 |

## Open

- `flatpak-builder` and `git-cliff` are not installed on the build machine; needed in S4.
- Not yet verified, see `docs/verified.md` "Plan changes" 10: Flathub acceptance of
  `--filesystem=xdg-documents` (S4), passing an activation token to a window opened later
  by the running GPUI app (S2), reading the Settings portal inside the sandbox (S4).
- `rust-version` goes from 1.91 to 1.92 when `krilla` is added for PDF export (S3).

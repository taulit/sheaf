# Release checklist: Sheaf 0.1.0

State on 2026-11-17, branch `t3/prepare-release-packaging` (on top of `phase/s4-release`).
Written to the template of plan 05 section 8. States: **done**, **open**, **blocked**.
The packaging itself is described in [packaging.md](packaging.md).

## 1. Requirements of plan 05 for Sheaf

### Budgets (05 section 1)

| Requirement | State | Evidence |
|---|---|---|
| Budgets of Sheaf and `sheaf-searchd`: cold start, RSS, search p95, scrolling, idle memory | measured by another session | See `docs/STATUS.md`, "Budgets". Known from S1: search p95 5.7 ms with 10,000 notes (budget 30 ms), `sheaf-searchd` about 11 MB idle (budget 15 MB) |
| Measurement script and `docs/performance.md` | open, same session | `scripts/budgets.sh` |
| Search budget checked in CI | done | `search-budget` job in `.github/workflows/ci.yml` |

### Tests and reviews (05 sections 2 to 4)

| Requirement | State | Evidence |
|---|---|---|
| `sheaf-core`, D-Bus, CLI, app and export tests | done | `cargo test --workspace --all-features`: 436 passed, 4 ignored (screenshots), 2026-11-17 (`docs/STATUS.md`) |
| Visor module tests | done | `scripts/test-visor-module.sh`: 23 of 23 |
| GNOME overview finds notes | done | `scripts/shell-search-check.sh`, `docs/screenshots/gnome-search-dark.png` |
| End-to-end scenarios (05 section 3) | open, other session | `scripts/e2e.sh` |
| App screenshots, light and dark | done | `scripts/screenshot.sh`, `docs/screenshots/app-*.png` |
| Review of S3 | done | 12 findings, all fixed (`docs/STATUS.md`) |
| Review of S4 (`bug-review`, `simplicity-review`, `spec-review`) | open | `docs/handoff.md`, "Next" |
| Checklist for the extension store (`docs/ego-checklist.md`) | open | Not written. The Visor module has to pass it before the upload (05 section 4) |
| Human read-through of the Visor module code | open, needs the user | The store rejects code that looks machine-written (05 section 4) |

### CI and releases (05 section 5)

| Requirement | State | Evidence |
|---|---|---|
| fmt, clippy, test, deny, search budget, Visor module | done, red | `.github/workflows/ci.yml`. Red because CI cannot fetch the private editor (user's decision, 2026-11-17); the gate runs locally |
| `appstreamcli validate`, `desktop-file-validate` in CI | open | They pass locally (`scripts/check-install.sh`). A CI job needs `ci.yml`, which this branch did not touch |
| `flatpak-builder` with artifact | blocked | `flatpak-bundle` job in `.github/workflows/release.yml`, off until the variable `FLATPAK_BUNDLE` is `true` |
| Visor module zip as artifact | done | `visor-module` job in `ci.yml` and `release.yml` |
| Release on a tag `vX.Y.Z`: tarball, notes, module zip | done, not run | `.github/workflows/release.yml` creates a draft. The workflow has never run on GitHub; YAML and the scripts were checked locally |
| Release notes from `git-cliff` | replaced | `git-cliff` is not installed. `CHANGELOG.md` is written by hand, `scripts/release-notes.sh` cuts a section out of it |

### Packages (05 section 6)

| Requirement | State | Evidence |
|---|---|---|
| Install layout used by all packages | done | `build-aux/install.sh`, checked by `scripts/check-install.sh` (14 files, service files, validators) |
| Metainfo with screenshots, releases, OARS, branding | done | `data/io.github.taulit.Sheaf.metainfo.xml`: `appstreamcli validate --explain --no-net` passes. Screenshot URLs work once the tag exists and the repository is public |
| Desktop file | done | `desktop-file-validate data/io.github.taulit.Sheaf.desktop` passes |
| Flatpak manifest | done, **untested** | `build-aux/flatpak/io.github.taulit.Sheaf.yml`, YAML parses. `flatpak-builder` is not installed here, so it has not been built or linted |
| `cargo-sources.json` | done | `scripts/update-cargo-sources.sh`: 1,867 sources from `Cargo.lock`, including the editor at `f7e001a` (tag `v0.0.3`) |
| AUR `sheaf` and `sheaf-git` | done, **unbuilt** | `build-aux/aur/*/PKGBUILD` and `.SRCINFO` (`makepkg --printsrcinfo`). `sha256sums` of `sheaf` is `SKIP` until release day. `namcap` is not installed |
| Zip of "Visor: Notes" | done | `scripts/pack-visor-module.sh`: 17 entries, `metadata.json` at the top, German `.mo` |
| Upload to extensions.gnome.org and the Visor catalog | open, needs the user | `docs/packaging.md`, "Visor module zip" |

### Findability (05 section 7)

| Requirement | State | Evidence |
|---|---|---|
| README in the order of the plan, cross-links to Visor and the editor | done | `README.md` (screenshots instead of a GIF) |
| GIF of the app | open | None recorded |
| GitHub description | open, needs the user | "Notes for GNOME – fast native Apple Notes–style app with plain Markdown files, gallery view and powerful search. Rust + GPUI." |
| GitHub topics | open, needs the user | `notes`, `markdown`, `gnome`, `gpui`, `rust`, `apple-notes`, `note-taking`, `linux` |
| Social preview 1280×640 | open | Make it from `docs/screenshots/app-gallery-*.png` |
| Cargo metadata (description, repository, homepage, keywords, categories) | done | `Cargo.toml` files. The crates are `publish = false`; nothing goes to crates.io |

## 2. Release day, in order

1. **Editor.** Decide which tag of `taulit/gpui-markdown-editor` to ship. Newer tags than
   `v0.0.3` exist (`v0.0.4`, `v0.1.0`, seen on 2026-11-17). Bump it in `Cargo.toml`, update
   `Cargo.lock`, run the gate. Make the editor repository public or give CI a token
   (blockers 1 and 2).
2. **Gate** on the final commit: `cargo fmt --check`, `cargo clippy --workspace
   --all-targets --all-features`, `cargo test --workspace --all-features`, `cargo deny check`,
   `scripts/test-visor-module.sh`, `scripts/budgets.sh`, `scripts/e2e.sh`,
   `scripts/check-install.sh`, `scripts/screenshot.sh` (refresh the screenshots if the UI
   changed).
3. **Version bump** (skip what is already right): `version` in `Cargo.toml`
   (`[workspace.package]`), `version-name` in `visor-module/metadata.json`, `pkgver` in
   `build-aux/aur/sheaf/PKGBUILD`, `tag:` in the Flatpak manifest.
4. **Dates and tag URLs.** In `CHANGELOG.md` set the date after the version to the day of the
   tag. In the metainfo set `<release version date>` to the same day and make the screenshot
   URLs name the tag. Run `scripts/check-release.sh <version>`; it must pass.
5. **Merge** to `main`, then tag and push: `git tag -a vX.Y.Z -m "Sheaf X.Y.Z"` and
   `git push origin vX.Y.Z`. The release workflow creates a draft release with the tarball, the
   Visor module zip and `SHA256SUMS`.
6. **Checksum into the AUR package.** Put the sha256 of `sheaf-X.Y.Z.tar.gz` into
   `build-aux/aur/sheaf/PKGBUILD`, run `makepkg --printsrcinfo > .SRCINFO`, commit. The
   tarball is the file of the tag, so the commit comes after the tag; the AUR copy is the only
   place that uses it.
7. **Cargo sources.** `scripts/update-cargo-sources.sh`; set `commit:` in the Flatpak manifest
   to the commit of the tag. Build and run it once with `flatpak-builder`, lint it with
   `flatpak-builder-lint`.
8. **Publish the draft release** on GitHub after reading it.
9. **Flathub.** Open a pull request against `flathub/flathub` (branch `new-pr`) with the
   manifest and `cargo-sources.json`. Read the current requirements first
   (<https://docs.flathub.org/docs/for-app-authors/requirements>). Explain
   `--filesystem=xdg-documents` and the exported search provider in the description.
10. **AUR.** Push `sheaf` and `sheaf-git` (`docs/packaging.md`, "AUR").
11. **Visor module.** Upload the zip at <https://extensions.gnome.org/upload/>; when it is
    accepted, add it to the catalog `taulit/visor-modules`.
12. **GitHub.** Description, topics, social preview. Install the Visor module locally and log
    in again to try it.

## 3. Known blockers

1. **The editor repository is private.** `taulit/gpui-markdown-editor` cannot be fetched by
   Flathub, by CI or by an AUR user. This stops: the Flatpak bundle job (`NEEDS EDITOR` in
   `release.yml`), the AUR build, the red `ci.yml`, and `scripts/update-cargo-sources.sh` on
   any machine without credentials. `cargo-sources.json` was generated here only because git
   had access. Flathub builds have no network and no credentials, so it needs a public
   repository (or a published crate).
2. **The Sheaf repository is private.** The screenshot URLs in the metainfo and the tarball
   URL of the AUR package return 404 until it is public.
3. **The Flatpak build is untested.** `flatpak-builder` is not installed and was not
   installed. Unverified: the build in the SDK (including `msgfmt`), the exported files, the
   search provider and the portals inside the sandbox.
4. **Still `[U]` in `docs/verified.md`:** Flathub's acceptance of `--filesystem=xdg-documents`,
   an activation token for a window opened later in the running app, the Settings portal
   inside the sandbox.
5. **The AUR packages have not been built** in a clean chroot, and `namcap` has not been run.
   Which libraries are needed at run time was read from `ldd` and from the names in the
   binary, not tested on a bare system.
6. **`shell-version` 50 and 51** of the Visor module: not checked against what the store
   accepts today.
7. **Budgets** are measured by another session: `docs/STATUS.md`, "Budgets".
8. **Open from `docs/STATUS.md`:** PDF limits, `/vorlage` in the slash menu, the app was not
   tried by hand on a real desktop session (no real portal dialog was opened).

## 4. What needs a human

- Make the repositories public (or decide to wait), publish the editor, create tokens.
- The final read of the Visor module code, the upload to extensions.gnome.org, the entry in
  `taulit/visor-modules`.
- The Flathub pull request and its review, including the decision on
  `--filesystem=xdg-documents`.
- AUR account, SSH key and the two pushes.
- GitHub description, topics and social preview.
- A run of `flatpak-builder` and a look at the result on a real session.

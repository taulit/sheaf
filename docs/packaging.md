# Packaging

How Sheaf is packaged, and how to build and check each package. What still has to happen
before a release is in [release-checklist.md](release-checklist.md).

## One layout

`build-aux/install.sh <prefix> [<destdir>]` installs a built Sheaf. Flatpak and both AUR
packages call it, so the layout exists once:

| What | Where (under `<prefix>`) |
|---|---|
| `sheaf`, `sheaf-searchd` | `bin/` |
| Desktop file | `share/applications/io.github.taulit.Sheaf.desktop` |
| Metainfo | `share/metainfo/io.github.taulit.Sheaf.metainfo.xml` |
| Icons | `share/icons/hicolor/scalable/apps/`, `share/icons/hicolor/symbolic/apps/` |
| D-Bus services | `share/dbus-1/services/io.github.taulit.Sheaf.service` and `….SearchProvider.service`, made from `data/*.service.in` with `Exec=<prefix>/bin/…` |
| Search provider | `share/gnome-shell/search-providers/io.github.taulit.Sheaf-search-provider.ini` |
| Translations | `share/locale/<lang>/LC_MESSAGES/sheaf.mo`, built from `po/` by `scripts/build-mo.sh` |
| Shell completions | bash, zsh and fish, written by `sheaf completions <shell>` |
| Licence | `share/licenses/$SHEAF_LICENSE_NAME/LICENSE` (default `sheaf`) |

`sheaf` comes from `$SHEAF_BUILD_DIR` (default `$CARGO_TARGET_DIR/release`, else
`target/release`), `sheaf-searchd` from `$SHEAF_SERVICE_BUILD_DIR` (default
`$CARGO_TARGET_DIR/service`, else `target/service`): the service is built with the Cargo
profile `service` (size-optimised, fat LTO) for its idle memory budget:

```sh
cargo build --release --locked -p sheaf
cargo build --profile service --locked -p sheaf-searchd
```
 `scripts/check-install.sh` installs stub binaries into a temporary prefix
and checks the tree, the service files, `desktop-file-validate` and `appstreamcli validate`.
It needs no build.

## Metainfo and desktop file

```sh
appstreamcli validate --explain --no-net data/io.github.taulit.Sheaf.metainfo.xml
desktop-file-validate data/io.github.taulit.Sheaf.desktop
```

`--no-net` because the screenshot URLs point at a release tag that does not exist yet (and
the repository is private). Without it, `appstreamcli` fetches the images; run it that way
once the tag is public.

- German texts are inline (`xml:lang="de"`). The repository has no gettext flow for XML
  (`scripts/update-pot.sh` covers the Rust sources only), so a new language is added to the
  file by hand.
- The only remaining note is `appstreamcli validate --pedantic`: the component ID has an
  uppercase letter. The ID is fixed by the app ID.
- The `<release>` date and the screenshot URLs (`…/v0.1.0/docs/screenshots/…`) change on
  release day, together. `scripts/check-release.sh <version>` checks that they agree.

## Flatpak

Manifest: `build-aux/flatpak/io.github.taulit.Sheaf.yml`. Runtime `org.freedesktop.Platform`
26.08 with the `rust-stable` extension (`docs/verified.md` section 4).
**The manifest has not been built**: `flatpak-builder` was not available when it was written.

Permissions: Wayland, X11 as fallback, `--share=ipc`, `--device=dri`,
`--filesystem=xdg-documents`. No network, no `--talk-name`, no `--own-name`
(`docs/verified.md` sections 3 and 4).

Visor inside Flatpak: Sheaf checks whether Visor supplies the Quick Note shortcut with
`org.gnome.Shell.Extensions.GetExtensionInfo`. That interface is exported only by the bus
name `org.gnome.Shell` at `/org/gnome/Shell` (checked with `gdbus introspect` on GNOME
Shell 50.5); the bus name `org.gnome.Shell.Extensions` belongs to the Extensions app
(`/usr/share/dbus-1/services/org.gnome.Shell.Extensions.service`), so it is no narrower
way in. Flatpak's finish-args cannot limit a `--talk-name` to one interface, and
`--talk-name=org.gnome.Shell` would open the whole shell to the sandbox, which Flathub
asks to justify. The manifest leaves it out: inside Flatpak the check always says "no
Visor", the shortcut is bound through the GlobalShortcuts portal, and the row "Shortcut from
the settings says so.

Sources: the app from its git tag, and `cargo-sources.json`, generated from `Cargo.lock`:

```sh
scripts/update-cargo-sources.sh
```

The script downloads `flatpak-cargo-generator.py` from `flatpak/flatpak-builder-tools` at a
pinned commit, checks its sha256, installs its two Python packages into a temporary virtual
environment and runs it with `python3 -I`. Nothing is installed on the system. Run it again
whenever `Cargo.lock` changes. The generator clones `taulit/gpui-markdown-editor`, so git
needs credentials for it while that repository is private.

Local build, with a checkout instead of the git source (edit the first source to
`type: dir`, `path: ../..`, as the comment in the manifest says):

```sh
flatpak install flathub org.freedesktop.Platform//26.08 org.freedesktop.Sdk//26.08 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08
flatpak-builder --user --install --force-clean build build-aux/flatpak/io.github.taulit.Sheaf.yml
flatpak run io.github.taulit.Sheaf
```

Lint, once the tools are installed:

```sh
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest build-aux/flatpak/io.github.taulit.Sheaf.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

Exported files: the desktop file, the icons, the metainfo, both D-Bus service files and the
search-provider ini. Flatpak rewrites `Exec=` in the service files so that
`sheaf-searchd` runs in the sandbox of the app. Flatpak marks exported search providers
`DefaultDisabled=true`, so users switch Notes on under Settings, Search (the README says so).

Not checked, and the first things to look at when the build runs: that the SDK has `msgfmt`
(the install script needs it), that the app finds its translations and the Settings portal
inside the sandbox, and that the search provider starts through its service file.

## AUR

`build-aux/aur/sheaf/` builds the release tarball, `build-aux/aur/sheaf-git/` the latest
commit. Each has a `PKGBUILD` and a `.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO`;
regenerate after every change of the PKGBUILD).

- `depends` come from `ldd` on the binary (`libxkbcommon`, `libxkbcommon-x11`, `libxcb`) and
  the library names it loads at run time (`libvulkan`, `libwayland-client`,
  `libwayland-egl`, `libEGL`). `fontconfig` and `freetype2` come from the package list of
  CI; `ldd` does not show them, so they may only be needed to build. Check this in a clean
  chroot.
- `check()` runs `cargo test --workspace --all-features` in the release profile, so it
  reuses the build. It needs `dbus-daemon` and `gettext`, both in `makedepends`.
- `package()` calls `build-aux/install.sh /usr "$pkgdir"`.
- `sheaf`: `sha256sums=('SKIP')` is a placeholder. On release day put the checksum of
  `sheaf-<version>.tar.gz` from the draft release (`SHA256SUMS`) there.
- `namcap` was not run (not installed). The packages have not been built in a clean chroot.
- `cargo fetch` needs the private editor repository, so an AUR build fails for everyone
  until it is public.

To publish (by a human, with an AUR account and SSH key):

```sh
git clone ssh://aur@aur.archlinux.org/sheaf.git aur-sheaf
cp build-aux/aur/sheaf/{PKGBUILD,.SRCINFO} aur-sheaf/
cd aur-sheaf && git add PKGBUILD .SRCINFO && git commit -m "sheaf 0.1.0" && git push
```

Same for `sheaf-git`.

## Visor module zip

`scripts/pack-visor-module.sh [dir]` writes
`sheaf-visor@taulit.github.io.shell-extension.zip` (default `target/`). It uses
`gnome-extensions pack`, with the compiled translations and the licence.

What the zip holds: `metadata.json`, `extension.js`, `stylesheet.css`, `lib/*.js`,
`locale/de/LC_MESSAGES/sheaf-visor.mo` and `LICENSE`, all at the top level, which is what
extensions.gnome.org expects. `metadata.json` has `uuid`, `name` ("Visor: Notes"),
`description` (it starts with "Requires Visor"), `shell-version` (50, 51), `url`,
`gettext-domain`, `version-name` and the `visor` key (`api` 1, slot `dock`). The store sets
the integer `version` itself, so the file has none.

Upload, by a human:

1. Sign in at <https://extensions.gnome.org/upload/> and upload the zip. Reviewers read the
   code, so look it over first (plan 05 section 4).
2. When it is accepted, add an entry for "Visor: Notes" to `taulit/visor-modules`
   (`modules/<uuid>.json`, icon, one to four screenshots of 1600×1000 that show the module
   inside Visor), as described in plan 01 section 11.

## Release workflow

`.github/workflows/release.yml` runs on a tag `vX.Y.Z`. It checks that the versions agree
(`scripts/check-release.sh`), builds the source tarball with `git archive` (the file the AUR
package checksums), runs the Visor tests and packs its zip, takes the notes from
`CHANGELOG.md` (`scripts/release-notes.sh`) and creates a **draft** GitHub release with
`SHA256SUMS`. It publishes nothing. The Flatpak bundle job is off until the editor can be
fetched; see the checklist.

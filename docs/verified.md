# Verified assumptions (phase S0)

Research date: 2026-11-16. Machine: Manjaro, GNOME Shell 50.5, `xdg-desktop-portal` 1.22.1, `xdg-desktop-portal-gnome` 50.0, `flatpak` 1.18.4, `ashpd` 0.14.0 and `zbus` 6.3.0 in the local cargo registry.

Legend: **[V]** verified from a primary source or a local command. **[S]** read in source code but not exercised end to end. **[U]** not verifiable here.

Raw upstream files were fetched from the `main` branches on 2026-11-16 (`flatpak/xdg-desktop-portal`, `GNOME/xdg-desktop-portal-gnome`, `GNOME/gnome-control-center`, `GNOME/gnome-shell`, `flatpak/flatpak`, `flathub/*`).

---

## 1. GlobalShortcuts portal

**Assumption.** Without the Visor extension, Sheaf registers Ctrl+Shift+N ("Quick Note") through `org.freedesktop.portal.GlobalShortcuts` while the app runs.

**Finding.**
- **[V] Present on this machine.** `gdbus introspect` on `org.freedesktop.portal.Desktop` lists `org.freedesktop.portal.GlobalShortcuts` with `version = 1`. `/usr/share/xdg-desktop-portal/portals/gnome.portal` lists `org.freedesktop.impl.portal.GlobalShortcuts`. `org.freedesktop.host.portal.Registry` (version 1) is present too.
- **[V] Backend history.** xdg-desktop-portal-gnome NEWS: "Add global shortcuts portal backend" in 48.rc; "Improvements to the Global Shortcuts portal" in 49.beta; "Properly send the Global Shortcut activation token" in 50.alpha; "Various fixes to the Global Shortcuts portal" in 51.rc. So it exists in 48, 49 and 50. The GNOME 48 developer notes call it fully supported.
- **[V] Interface version.** The upstream XML on `main` documents version 2, which adds `ConfigureShortcuts`. The installed portal here reports version 1. ashpd's `configure_shortcuts` calls `call_versioned(..., 2)`. Check `GlobalShortcuts::version()` before using it.
- **[S] Where the dialog comes from.** The GNOME backend (`src/globalshortcuts.c`) forwards `BindShortcuts` to `org.gnome.Settings.GlobalShortcutsProvider`. That is the `gnome-control-center-global-shortcuts-provider` binary, which shows the dialog. The backend converts `preferred_trigger` such as `CTRL+a` to `<ctrl>a`.
- **[S] Dialog behaviour.** `cc_global_shortcut_dialog_present()` emits "done" without any UI when `has_new_shortcuts` is false. The dialog is therefore shown only when the app offers shortcuts that are not yet saved for its app ID. The user can change or reject the trigger. The saved shortcuts are kept by the control-center keyboard manager and are listed in Settings, Apps, Sheaf. A GNOME maintainer confirmed on Discourse that the dialog re-appears when an app re-binds shortcuts it should not (Chrome bug). Sheaf must bind a fixed, small set exactly once per session.
- **[S] Persistence.** Saved bindings persist across restarts, and a later `BindShortcuts` with the same IDs shows no dialog. `ListShortcuts` before any bind returns the shortcuts bound earlier by this app (portal XML).
- **[V] Only while a session lives.** Shortcuts belong to a session, and a session belongs to the D-Bus peer that created it (`peer-name` in `globalshortcuts.c`). `Activated` is emitted only to that peer, so the shortcut works only while Sheaf runs and holds the session. `BindShortcuts` can be called only once per session ("Session already has bound shortcuts").
- **[V] Host apps need an app ID.** The portal normally takes the app ID from the systemd scope name (`app-<id>-….scope`). D-Bus-activated apps (`dbus-…` units) and apps started from a terminal get an **empty** app ID (Ignacy Kuchciński, https://blogs.gnome.org/ignapk/?p=69). In control-center's provider, a bind or configure request with an empty app ID is discarded ("Discarded shortcut bind request from application with an invalid app_id"). The fix is `org.freedesktop.host.portal.Registry.Register(app_id, {})`. The ID must match the basename of an installed `.desktop` file. It must be the **first** call to any portal on that D-Bus connection and can be done only once. It does not work for sandboxed apps (Flatpak provides the ID itself).
- **[V] ashpd 0.14.0 API.**
  - `ashpd::register_host_app(AppID)` is a no-op in a sandbox; `register_host_app_with_connection` also exists. Feature: none beyond the default.
  - Feature `global_shortcuts`, module `ashpd::desktop::global_shortcuts`.
  - Calls: `GlobalShortcuts::new()` / `with_connection()`, `version()`, `create_session(CreateSessionOptions)`, `bind_shortcuts(&session, &[NewShortcut], Option<&WindowIdentifier>, BindShortcutsOptions)`, `list_shortcuts`, `configure_shortcuts` (v2), `receive_activated()`, `receive_deactivated()`, `receive_shortcuts_changed()`.
  - Types: `NewShortcut::new(id, description).preferred_trigger(Some("CTRL+SHIFT+n"))`, `Shortcut`, `Activated` (`shortcut_id()`, `timestamp()`, `options()` with `activation_token`), `Session<GlobalShortcuts>`.
  - Pass the same zbus connection to `register_host_app_with_connection` and `GlobalShortcuts::with_connection`, so the registered peer is the one that creates the session.
- **[V] Trigger format.** `preferred_trigger` follows the freedesktop shortcuts spec: modifiers `CTRL`, `ALT`, `SHIFT`, `NUM`, `LOGO`, joined with `+`, then an XKB keysym name, for example `CTRL+SHIFT+n`. The compositor may ignore it; always display `trigger_description` from the bind response.
- **[V] Activation token.** `Activated` options may carry `activation_token`. gpui-pre 0.3.13 `PlatformInit` has an `activation_token` field (from `XDG_ACTIVATION_TOKEN` in the environment at startup, `src/platform.rs:244`). Whether it can be changed for an already running GPUI app is **[U]**. Test it in the spike.
- **[U] AUR/host install end to end.** I did not create a session or bind a shortcut, because that would open a dialog and change user settings. The chain above comes from the portal docs and backend source.

**Decision for Sheaf.**
1. On startup (non-sandboxed only): create one zbus connection, call `register_host_app_with_connection(conn, "io.github.taulit.Sheaf")` **before any other portal call** (including Settings, Notification, FileChooser). That means before the first Settings read for the theme.
2. Install `io.github.taulit.Sheaf.desktop` (AUR: `/usr/share/applications`; Flatpak: exported automatically). Keep the desktop file ID equal to the app ID.
3. Create one session, bind one shortcut `quick-note` with `preferred_trigger("CTRL+SHIFT+n")` and description "Quick note". Bind once per run. Do not re-bind on every start if `list_shortcuts` already returns it.
4. On `Activated`: open the quick-note window, passing the `activation_token` when GPUI allows it. Fall back to normal focus otherwise.
5. If `GlobalShortcuts::new()` fails, the proxy version is 0, or `bind_shortcuts` returns no shortcuts: hide the in-app shortcut row and show the fallback. Offer "Configure shortcut" (`configure_shortcuts`) only when the portal version is 2 or higher.
6. Fallback (documented in Settings): a GNOME custom keybinding in Settings, Keyboard, Custom Shortcuts with command `sheaf --quick` (single-instance handoff, section 6). For Flatpak: `flatpak run io.github.taulit.Sheaf --quick`. If Visor is installed, it supplies the shortcut instead and Sheaf skips the portal.

---

## 2. PDF export and printing

**Assumption.** "PDF via the portal print dialog, otherwise HTML → PDF with a lean Rust crate."

**Finding.**
- **[V] The portal prints files, it does not render.** `org.freedesktop.portal.Print` (version 4 here) has two methods (https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Print.html):
  - `PreparePrint(parent_window, title, settings, page_setup, options) -> request`. Shows the print dialog. The response carries `settings`, `page-setup` (paper size and margins in mm, orientation) and a `token` (`u`).
  - `Print(parent_window, title, fd, options) -> request`. `fd` is a file descriptor to read the content to print. With `options.token` from a previous `PreparePrint`, no dialog appears. Without a token, the dialog is shown at this point.
  - Option `supported_output_file_formats` (`as`, values `pdf`, `ps`, `svg`, added in v3) tells the dialog which print-to-file formats the app can handle.
- **[S] What the fd must contain.** xdg-desktop-portal-gnome `src/print.c` passes the fd to `gtk_print_job_set_source_fd()` (GtkPrintJob). GTK needs a PDF or PostScript stream. It also offers print preview when `papers-previewer` or `evince-previewer` is installed. The GNOME dialog (GtkPrintUnixDialog) lists "Print to File", and `print.c` sets `GTK_PRINT_CAPABILITY_GENERATE_PDF` when the app lists `pdf`.
- **[V] Consequence for GPUI.** There is no GtkPrintOperation and no WebKit. Sheaf must produce the complete PDF itself, after the user has chosen paper size and margins in the dialog. The portal never rasterises or lays out anything.
- **[V] ashpd 0.14.0 API.** Feature `print`, module `ashpd::desktop::print`: `PrintProxy::new()`, `prepare_print(identifier, title, Settings, PageSetup, PreparePrintOptions)` returning `Request<PreparePrint>` (settings, page setup, token), `print(identifier, title, fd, PrintOptions)`. `Settings` and `PageSetup` have builder setters (`set_paper_format`, `set_orientation`, `set_width`, …).
- **PDF crate candidates** (crates.io API, 2026-11-16):

| | krilla | printpdf | genpdf | typst (+ typst-pdf) |
|---|---|---|---|---|
| Version / last release | 0.8.2 / 2026-06-04 | 0.12.9 / 2026-11-11 | 0.2.0 / 2021-06-17 | 0.15.1 / 2026-07-17 |
| License | MIT OR Apache-2.0 | MIT | Apache-2.0 OR MIT | Apache-2.0 |
| MSRV | 1.92 | 1.88 | none set | 1.92 |
| GPL-3.0-or-later compatible | yes | yes | yes | yes (Apache-2.0 into GPLv3 is one-way compatible) |
| Maintained | yes (Typst ecosystem) | yes | **no** (4+ years) | yes |
| Layout | none (draw primitives, you lay out) | low-level ops; optional `html` feature (azul-layout), README calls it "still evolving" | simple flow layout, tables, lists | full layout engine |
| Text shaping | `simple-text` feature (rustybuzz in the released 0.8.2; `main` has moved to harfrust); or pass pre-shaped glyphs | `allsorts` (shaping) | `rusttype`, no real shaping | full |
| Font embedding | subsetting, TTF and CFF, colour fonts | yes | yes (via printpdf 0.3-era) | yes |
| Images | PNG, JPEG, GIF, WebP; SVG via `krilla-svg` | many formats (feature flags) | via `image` | yes |
| Deps (normal, non-optional) | 14 small crates (pdf-writer, skrifa, subsetter, tiny-skia-path, flate2 …) | lopdf, allsorts-azul, serde, time, wasm-bindgen … | printpdf, lopdf, rusttype | typst-library alone has 59 direct deps; typst-pdf pulls in krilla anyway |
| Fit | best base for a custom renderer | usable, heavier, HTML path immature | dead | wrong shape: needs a `World`, markup generation and fonts; very heavy |

  krilla's own README says it expects the caller to supply laid-out content and names Typst as the option when you want automatic layout. Its tests lay out text with `parley`.
- **[V] Layout dependency.** `parley` 0.13.0 (2026-11-12, Apache-2.0 OR MIT, from Linebender) does shaping, bidi and line breaking and is what krilla's tests use. `cosmic-text` 0.19.0 is already in the local registry via GPUI and could serve instead. Which one GPUI-component's markdown renderer already depends on was not checked.

**Decision for Sheaf.** Use **krilla** (default features plus `simple-text`, no `rayon` at first) and write a small block layouter in `sheaf-core` or a new `sheaf-export` crate: block model in, pages out, using parley or cosmic-text for line breaking. It must handle headings, paragraphs, bold/italic runs (separate font faces), lists, check lists (draw boxes as paths), tables (fixed column widths, cell wrap), images, code blocks, and page breaks. Embed a bundled font for Latin and umlauts (for example Noto Sans or Inter, SIL OFL); krilla subsets it, so file size stays small. German umlauts need no special handling beyond a font that contains them. Do not adopt printpdf, genpdf or Typst. Rust MSRV: krilla 0.8.2 needs 1.92; the workspace currently says `rust-version = "1.91"`, so raise it (the runtime Rust extension ships 1.98, section 4).

Flows:
- **Export PDF:** FileChooser portal (save dialog, suggested name `<title>.pdf`, ashpd `file_chooser`) → render PDF with the app's default page setup (A4, 20 mm margins, or locale default) → write to the chosen URI. No Print portal involved.
- **Print:** `PreparePrint` (title, default A4 page setup, `supported_output_file_formats: ["pdf"]`) → read `page-setup`, `settings` (copies, ranges, orientation) and `token` from the response → render a PDF for exactly that paper size and margins → write to a memfd or a temporary file → `Print(fd, token)`. The user can pick "Print to File" in the GNOME dialog as well; that is a bonus, not the export path.
- Fallback when the Print portal is missing: show only Export PDF, plus "Open in viewer" to print from there.

---

## 3. Flatpak, GNOME Shell search provider, D-Bus services

**Assumption.** Flatpak exports `.desktop`, icons, metainfo, D-Bus service files and `search-provider.ini`; `sheaf-searchd` owns `io.github.taulit.Sheaf.SearchProvider`, is D-Bus activated and implements `org.gnome.Shell.SearchProvider2`.

**Finding.**
- **[V] Export rules** (Flatpak docs https://docs.flatpak.org/en/latest/conventions.html, and `flatpak/common/flatpak-dir.c` / `flatpak-context.c` on `main`). Files inside `/app` are exported only from these directories, with these extensions and name rules:

| Directory | Extension | Name must be |
|---|---|---|
| `share/applications` | `.desktop` | `$FLATPAK_ID` or `$FLATPAK_ID.*` (also `-foo`) |
| `share/icons/**` | `.png .svg .svgz .ico` | prefixed by `$FLATPAK_ID` |
| `share/dbus-1/services` | `.service` | **exactly** the bus name: `$FLATPAK_ID`, `$FLATPAK_ID.*` or a name granted by `--own-name`; the `Name=` key must equal the file name without `.service` (else "D-Bus service file has wrong name" and export fails) |
| `share/gnome-shell/search-providers` | `.ini` | `$FLATPAK_ID*` (docs: `$NAME-search-provider.ini`, `$NAME` = ID or subname) |
| `share/metainfo` | `.xml` | `$FLATPAK_ID.metainfo.xml` |

  Files with other names or extensions are removed at export time with a warning.
- **[V] `Exec=` rewriting.** `export_desktop_file()` rewrites `Exec=` in `.desktop` **and** `.service` files to `flatpak run --branch=<b> --arch=<a> --command=<old argv[0]> <app-id> <old args…>`. So a second binary works: `Exec=/app/bin/sheaf-searchd` becomes `--command=/app/bin/sheaf-searchd`. Each D-Bus service file may name a different command. For `.service` files only the keys `Exec` and `Name` are kept (`allowed_dbus_service_keys`), so `SystemdService=` and `User=` are dropped.
- **[V] Search provider files get `DefaultDisabled=true`.** `export_ini_file()` sets `DefaultDisabled=true` in `[Shell Search Provider]` for every exported search-provider ini (docs: "Flatpak will mark all search providers files as disabled when exporting them"). GNOME Shell reads it (`js/ui/remoteSearch.js`: `remoteProvider.defaultEnabled = !keyfile.get_boolean(group, 'DefaultDisabled')`). **Flatpak users must switch Sheaf on once in Settings, Search.** Non-Flatpak installs are enabled by default.
- **[V] Own-name.** Flatpak grants `$FLATPAK_ID` (and `$FLATPAK_ID.*` for non-`--sandbox` runs) on the session bus automatically (`flatpak-context.c`, `--own=%s.*`). The Flathub linter rejects `--own-name=<FLATPAK_ID>` as `finish-args-unnecessary-appid-own-name` ("never granted"). `io.github.taulit.Sheaf.SearchProvider` is under that wildcard, so **no `--own-name` is needed**. The search provider process runs in its own sandbox instance with the same app ID and permissions.
- **[V] Shell-side requirements** (`gnome-shell/js/ui/remoteSearch.js`). The ini must have group `[Shell Search Provider]` with keys `DesktopId` (required; `GioUnix.DesktopAppInfo.new` must succeed and `should_show()` must be true, so no `NoDisplay=true`), `BusName`, `ObjectPath`, `Version` (`2` for SearchProvider2; absent means 1), optional `AutoStart` (default true; if false the shell does not start the service), optional `DefaultDisabled`. If two inis share an `ObjectPath`, the later one is ignored. The shell scans `search-providers` in every XDG data dir.
- **[V] Non-Flatpak install location.** `$prefix/share/gnome-shell/search-providers/<name>-search-provider.ini` (system examples: `/usr/share/gnome-shell/search-providers/org.gnome.Calculator-search-provider.ini` with `DesktopId=org.gnome.Calculator.desktop`, `BusName=org.gnome.Calculator.SearchProvider`, `ObjectPath=/org/gnome/Calculator/SearchProvider`, `Version=2`). The shell reads search providers only from the directories in `XDG_DATA_DIRS`, not from `XDG_DATA_HOME`: a provider in `~/.local/share/gnome-shell/search-providers/` is not found (confirmed in the headless shell check of phase S1, `scripts/shell-search-check.sh`). The D-Bus service file goes to `$prefix/share/dbus-1/services/io.github.taulit.Sheaf.SearchProvider.service`.
- **[V] `org.gnome.Shell.SearchProvider2` signatures** (introspection XML in `remoteSearch.js`):
  - `GetInitialResultSet(in as terms, out as results)`
  - `GetSubsearchResultSet(in as previous_results, in as terms, out as results)`
  - `GetResultMetas(in as identifiers, out aa{sv} metas)`
  - `ActivateResult(in s identifier, in as terms, in u timestamp)`
  - `LaunchSearch(in as terms, in u timestamp)`
- **[V] Result meta keys read by Shell 50** (`getResultMetas`): `id` (s, required), `name` (s), `description` (s, optional), `clipboardText` (s, optional), and the icon by priority: `icon` (a serialized `GIcon`, variant from `g_icon_serialize`, kept as variant) > `gicon` (s, a `GIcon` string such as an icon name or file path) > `icon-data` ((iiibiiay): width, height, rowstride, has-alpha, bits-per-sample, n-channels, data). If none is given the result has no icon. All other values are unpacked with `deepUnpack()`, so send plain types.
- **[V] D-Bus activation of the provider.** The shell proxies the bus name with auto-start unless `AutoStart=false`, so the first search starts `sheaf-searchd` through its `.service` file. `ActivateResult` runs in `sheaf-searchd`; it must hand the note ID to the UI app (single-instance handoff, section 6), not open a window itself.
- **[V] Flathub policy.**
  - The Flathub requirements page (https://docs.flathub.org/docs/for-app-authors/requirements) says static permissions "must be kept to an absolute minimum" and that portals must be used when a suitable one exists, with exceptions case by case. I found no Flathub page that mentions search providers specifically, and the linter has no rule against exporting them.
  - `--filesystem=xdg-documents` is not in the linter's rule list (the listed rules concern home, host, `.config`, `.cache`, `.local`, absolute paths, `xdg-data/config/cache` with modes). It is therefore not blocked mechanically, but a reviewer may still ask for the FileChooser portal with a persistent document-portal grant. Whether review accepts `xdg-documents` for a notes app is **[U]**; decide by asking in the submission.
  - `--share=ipc` is required whenever X11 is used (`finish-args-x11-without-ipc`), and `fallback-x11` needs `--socket=wayland` (`finish-args-fallback-x11-without-wayland`).

**Decision for Sheaf.**
- Flatpak files (all names verified against the rules above):
  - `/app/share/applications/io.github.taulit.Sheaf.desktop` (`DBusActivatable=true`, `Exec=sheaf %U`-style fallback, no `NoDisplay`)
  - `/app/share/dbus-1/services/io.github.taulit.Sheaf.service` with `Name=io.github.taulit.Sheaf`, `Exec=/app/bin/sheaf --gapplication-service` (use our own flag, for example `--dbus-service`)
  - `/app/share/dbus-1/services/io.github.taulit.Sheaf.SearchProvider.service` with `Name=io.github.taulit.Sheaf.SearchProvider`, `Exec=/app/bin/sheaf-searchd`
  - `/app/share/gnome-shell/search-providers/io.github.taulit.Sheaf-search-provider.ini` with `DesktopId=io.github.taulit.Sheaf.desktop`, `BusName=io.github.taulit.Sheaf.SearchProvider`, `ObjectPath=/io/github/taulit/Sheaf/SearchProvider`, `Version=2`
  - metainfo `io.github.taulit.Sheaf.metainfo.xml`; icons `io.github.taulit.Sheaf*.svg|png`
- No `--own-name` in finish-args. Document in the README that Flatpak users must enable Sheaf under Settings, Search.
- `sheaf-searchd` is built as a normal binary, installed to `/app/bin`, and shares the tantivy index directory with the app (same app ID, same `~/.var/app/<id>/data`).
- Use the portal route for the notes folder where possible. If a plain filesystem path is needed for tantivy and `notify`, request `--filesystem=xdg-documents` and justify it in the submission.

---

## 4. Flatpak runtime and GPUI finish-args

**Assumption.** Runtime `org.freedesktop.Platform` 25.08 plus the Rust SDK extension.

**Finding.**
- **[V] 26.08 is released.** `flatpak remote-ls flathub --runtime` on this machine lists `org.freedesktop.Platform` and `Sdk` at branches `25.08` and `26.08`, and `org.freedesktop.Sdk.Extension.rust-stable` at `25.08` and `26.08`. The Flathub rust-stable repo has a `branch/26.08` whose commit "Bump from 26.08beta to 26.08" is dated 2026-09-03. `org.gnome.Platform` 49, 50 and 51 exist. Zed's Flathub manifest (`flathub/dev.zed.Zed`, `dev.zed.Zed.yaml`) already uses `runtime-version: '26.08'`.
- **[V] Rust extension.** ID `org.freedesktop.Sdk.Extension.rust-stable`, installed under `/usr/lib/sdk/rust-stable`. Its manifest on `branch/26.08` downloads `rust-1.98.0` (dist of 2026-08-20); `branch/25.08` was updated to `1.98.1` on 2026-09-03. Both meet rustc ≥ 1.91 and also krilla's 1.92. Use `sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]`, `build-options.append-path: /usr/lib/sdk/rust-stable/bin`, and an offline vendored build (`cargo --offline` with sources generated by `flatpak-cargo-generator`); Flathub builds have no network.
- **[V] Zed's finish-args** (https://github.com/flathub/dev.zed.Zed/blob/master/dev.zed.Zed.yaml): `--device=dri`, `--share=ipc`, `--socket=fallback-x11`, `--socket=wayland`, plus app-specific ones (`--share=network`, pulseaudio, filesystem). GPUI renders with wgpu on Linux; `wgpu 30.0.2` is in the local registry. That matches the plan's finish-args.
- **[V] Linter constraints.** `fallback-x11` requires `wayland`; `x11` together with `fallback-x11` is rejected; X11 needs `--share=ipc`.

**Decision for Sheaf.**
- `runtime: org.freedesktop.Platform`, `runtime-version: '26.08'`, `sdk: org.freedesktop.Sdk`, `sdk-extensions: [org.freedesktop.Sdk.Extension.rust-stable]`. Fall back to 25.08 only if a build problem appears.
- finish-args: `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`, `--device=dri`, plus the notes-folder permission from section 3. No `--share=network`, no `--talk-name` for portals (portals are reachable by default), no `--own-name`.
- Add `/app/libflatpak`-style tricks only if a library lands in `/app/lib` (Zed does this for its host-command escape; Sheaf does not need it).

---

## 5. Settings portal

**Assumption.** Theme from `org.freedesktop.appearance` (`color-scheme`, `accent-color`, `contrast`, live), UI font from `org.gnome.desktop.interface` `font-name` and `text-scaling-factor`, all through the portal.

**Finding.**
- **[V] Standard keys** (portal XML, version 2, https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Settings.html), namespace `org.freedesktop.appearance`:
  - `color-scheme` (`u`): 0 no preference, 1 prefer dark, 2 prefer light; unknown values count as 0.
  - `accent-color` (`(ddd)`): sRGB in [0,1]; out-of-range means unset.
  - `contrast` (`u`): 0 normal, 1 higher.
  - `reduced-motion` (`u`): 0 no preference, 1 reduced motion. (Not in the plan; worth honouring for animations.)
  - Local `ReadAll` result: `contrast u 0`, `color-scheme u 1`, `reduced-motion u 0`, `accent-color (ddd) 0.0862745 0.627451 0.521569`.
- **[V] Methods and signal.** `ReadAll(as namespaces) -> a{sa{sv}}` (trailing-`*` globbing), `Read` (deprecated, double-wrapped variant), `ReadOne` (v2), signal `SettingChanged(s namespace, s key, v value)`.
- **[V] GNOME namespaces.** xdg-desktop-portal-gnome `src/settings.c` exposes `org.gnome.desktop.interface` among its schema list (also `a11y`, `a11y.interface`, `calendar`, `input-sources`, `peripherals.mouse`, `privacy`, `sound`, `wm.preferences`, `settings-daemon.plugins.xsettings`) plus a synthetic `org.gnome.fontconfig`, and re-emits `SettingChanged` for them. Local reads on this machine: `ReadOne org.gnome.desktop.interface font-name` returns `s "Arial 11"`; `text-scaling-factor` returns `d 1`; `ReadAll org.gnome.desktop.interface` returns 45 keys. The portal is the same service for host and Flatpak apps; the Settings frontend only forwards to the backend. I did not run a read inside a sandbox **[U]**.
- **[V] Key detail.** `font-name` is a GTK-style font description string (family plus size in points, for example `Arial 11`), `document-font-name` and `monospace-font-name` exist as well. Treat `text-scaling-factor` as a multiplier (double).
- **[V] ashpd 0.14.0 API.** Feature `settings` (module `ashpd::desktop::settings`): `Settings::new()`, `read_all(&[ns])`, `read::<T>(ns, key)` (handles the double-wrapped variant), helpers `color_scheme()` -> `ColorScheme { NoPreference, PreferDark, PreferLight }`, `accent_color()` -> `Color` (r, g, b f64), `contrast()`, `reduced_motion()`, streams `receive_color_scheme_changed()`, `receive_accent_color_changed()`, `receive_contrast_changed()`, `receive_reduced_motion_changed()`, generic `receive_setting_changed()` and `receive_setting_changed_with_args::<T>(ns, key)`. ashpd's `read` calls the deprecated `Read`, which still works.

**Decision for Sheaf.**
- Startup: `read_all(["org.freedesktop.appearance", "org.gnome.desktop.interface"])` once, then subscribe with `receive_setting_changed()` and filter by namespace and key (one stream, fewer proxies). Map `color-scheme` 1 to dark, 2 to light, 0 to light (or follow `GNOME 50` default). Use `accent-color` when it is in range, else the Sheaf default.
- Parse `font-name` by splitting the last token as size in points; UI font size = size × `text-scaling-factor`. Keep monospace from `monospace-font-name`.
- When the portal is absent, fall back to `gsettings` defaults and the light theme without error.
- Register the host app (section 1) before the first Settings call.

---

## 6. D-Bus single instance (`org.freedesktop.Application`)

**Assumption.** Further launches hand their arguments to the running instance; GNOME Shell can launch Sheaf by D-Bus activation.

**Finding.**
- **[V] Interface** (Desktop Entry Specification, section 8, https://specifications.freedesktop.org/desktop-entry/latest/dbus.html):
  - `Activate(in a{sv} platform_data)`: called when started without files.
  - `Open(in as uris, in a{sv} platform_data)`: called when started with files; URIs are UTF-8.
  - `ActivateAction(in s action_name, in av parameter, in a{sv} platform_data)`: for Desktop Actions.
- **[V] Naming.** The desktop file name minus `.desktop` is the well-known bus name (`io.github.taulit.Sheaf`), and the app must have a D-Bus service **activatable** at that name. The object path is the bus name with dots changed to slashes, a leading slash, and any `-` changed to `_`: `/io/github/taulit/Sheaf`.
- **[V] `platform_data` fields:** `desktop-startup-id` (value of `DESKTOP_STARTUP_ID`, startup-notification protocol) and `activation-token` (value of `XDG_ACTIVATION_TOKEN`, Wayland xdg-activation) as strings in the variant dict. The app should use the token to take focus for the window it shows.
- **[V] Desktop key.** `DBusActivatable=true` (boolean, default false): implementations should ignore `Exec` and send a D-Bus message to launch. `Exec=` should still be present for implementations that do not understand the key, and is required if `DBusActivatable` is not true. Desktop Actions (`Actions=quick-note;`, `[Desktop Action quick-note]`) arrive as `ActivateAction`.
- **[V] Service file.** `[D-BUS Service]` with `Name=io.github.taulit.Sheaf` and `Exec=…`. When activation starts the process, D-Bus calls `Activate`/`Open` right after the name appears, so the app must request the name and export the object **before** the first GUI work, and queue calls that arrive before the UI is ready. (GApplication does the same with `--gapplication-service`; a non-GTK app picks its own flag.)
- **[V] Portal side effect.** D-Bus-activated apps run in a `dbus-…` systemd unit, so the portal sees no app ID. The Registry call (section 1) is therefore mandatory for host installs.
- **[V] zbus.** zbus 6.3.0 (local registry) provides `#[zbus::interface]` with `a{sv}` as `HashMap<String, OwnedValue>` and `connection::Builder::session()?.name("io.github.taulit.Sheaf")?.serve_at("/io/github/taulit/Sheaf", Application)?`. Use `DBusError`-less methods returning `()`. The request flag `DoNotQueue` makes a second instance fail fast instead of waiting: on failure, call `Activate`/`Open` on the existing instance and exit.
- **[U] Focus.** gpui-pre 0.3.13 reads `XDG_ACTIVATION_TOKEN` from the environment at startup (`PlatformInit.activation_token`). I did not find or test an API to hand a token to a window opened later in a running app. Verify in the spike; if missing, set the env var before opening the window or accept that GNOME may only flash the taskbar entry.

**Decision for Sheaf.**
- The main binary owns `io.github.taulit.Sheaf` at `/io/github/taulit/Sheaf` and implements `org.freedesktop.Application`. A second launch (`sheaf`, `sheaf note.md`, `sheaf --quick`) first tries to own the name. If it is taken, it calls `Activate`, `Open`, or `ActivateAction("quick-note", …)` with `activation-token` taken from its own environment, then exits.
- `sheaf --quick` maps to `ActivateAction("quick-note")`; the desktop file declares `Actions=quick-note;` so the GNOME app menu offers it too.
- Desktop file: `DBusActivatable=true` plus a regular `Exec=sheaf %U`.
- `sheaf-searchd`'s `ActivateResult` calls `Open` with a `sheaf://note/<id>` style URI (or `ActivateAction("open-note", [id])`) on the main instance, passing along the `timestamp` it received.

---

## Plan changes

1. **Global shortcut needs the host Registry call.** *Plan:* register through the portal while the app runs. *Now:* host (AUR) installs must call `org.freedesktop.host.portal.Registry.Register("io.github.taulit.Sheaf")` first, before any other portal call, and ship a matching `.desktop` file. Without it D-Bus-activated or terminal launches get an empty app ID and GNOME's provider discards the request. A one-time dialog appears for new shortcuts; ConfigureShortcuts needs portal version 2, and this machine only has version 1.
2. **Search provider is off by default in Flatpak.** *Plan:* the Flatpak exports the provider and it works. *Now:* Flatpak sets `DefaultDisabled=true`; the user enables Sheaf under Settings, Search. Document it.
3. **No `--own-name` for the search provider.** *Plan:* an own-name may be needed. *Now:* not needed, and the app-ID one is rejected by the Flathub linter.
4. **Service file names are strict.** Each file in `share/dbus-1/services` must be named exactly like the `Name=` key; names must be the app ID or start with `<app-id>.`.
5. **Runtime is 26.08, not 25.08.** *Plan:* `org.freedesktop.Platform` 25.08. *Now:* 26.08 is stable (Rust extension ships rustc 1.98.0; 25.08 has 1.98.1). rustc ≥ 1.91 is met on both.
6. **PDF: no HTML route.** *Plan:* portal print dialog, otherwise HTML → PDF. *Now:* the portal only prints a finished PDF/PS file, so Sheaf always renders the PDF itself with **krilla** (0.8.2, MIT OR Apache-2.0) plus its own layouter (parley or cosmic-text for line breaking). Export PDF writes the file directly via the FileChooser portal; Print is `PreparePrint` → render for the chosen page setup → `Print(fd, token)`. No HTML pipeline, no printpdf/genpdf/Typst.
7. **MSRV.** krilla 0.8.2 needs rustc 1.92; the workspace says `rust-version = "1.91"`. Raise it to 1.92.
8. **Settings read order.** The portal Settings read must come after the Registry call, and the font comes from `font-name` × `text-scaling-factor`, with `reduced-motion` added to the appearance keys.
9. **Single-instance activation must claim the name early.** The main binary is the D-Bus activatable service for `io.github.taulit.Sheaf`; searchd forwards `ActivateResult` to it instead of opening anything itself.
10. **Open items that remain [U].** `--filesystem=xdg-documents` acceptance on Flathub, passing an activation token to a window opened later in a running GPUI app, and a Settings-portal read from inside a sandbox. Test them in the S0 spike.

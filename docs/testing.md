# Testing

## GNOME Shell search provider check

```sh
cargo build --workspace
scripts/shell-search-check.sh [--scheme dark|light] [--size WxH] [QUERY]
```

Needs `gnome-shell` 50, `dbus-run-session`, `gdbus`, `busctl` and `jq`. `QUERY` defaults to `Milch`.
A run takes about 4 seconds. Screenshots go to `docs/screenshots/gnome-search-<scheme>.png`, the
shell log to `.shell-test/search-<scheme>.log`.

### What it proves

1. The shell finds `io.github.taulit.Sheaf-search-provider.ini`, resolves its `DesktopId` to the
   installed desktop file and lists Sheaf as a provider ("Notes").
2. Typing in the overview starts `sheaf-searchd` through its D-Bus service file, and the shell
   shows the note "Milch kaufen" (asserted by title) among the results. The screenshot shows it.
3. The CLI works against the live service: `sheaf new` followed at once by `sheaf search --json`
   returns the new note (D-Bus `Sync()` plus the on-disk index), and the session bus shows
   `io.github.taulit.Sheaf.SearchProvider` as owned.

On failure the script exits non-zero and prints the results the shell did show and the log path.

### What the sandbox isolates

`scripts/shell-sandbox.sh -- COMMAND` starts a headless shell (`--headless --wayland --virtual-monitor`)
under `env -i` with its own `HOME`, XDG directories, runtime directory and D-Bus session in
`$XDG_RUNTIME_DIR/sheaf-shell.*`. The real session bus, dconf settings and `~/.local/share` are never
touched. Inside it:

- the desktop file, the icon (if `data/icons/...` has one), the search-provider ini and both D-Bus
  service files (`@bindir@` replaced by `target/debug`) are installed into the sandbox data dir,
  which is also put in `XDG_DATA_DIRS` (the shell scans search providers only there);
- `target/debug` is on `PATH`, `SHEAF_NOTES_DIR` is an empty folder of the sandbox;
- `tests/shell-helper` is loaded as a test-only extension that exports `Eval` and `Screenshot`
  on the sandbox bus (the shell's own methods are closed to outside tools);
- on exit every process that carries the sandbox environment is stopped and the directory removed.

The overview is always dark in GNOME Shell, so `--scheme light` changes only the colour-scheme setting;
the two screenshots differ little.

## Visor module

`visor-module/` is the GNOME Shell extension "Visor: Notes" (`sheaf-visor@taulit.github.io`). It plugs a
Notes module into [Visor](https://github.com/taulit/visor) through its module API (tag `api-v1`) and talks to
`sheaf-searchd` over `io.github.taulit.Sheaf1`: a dock view with a field for new notes and the six most recent
notes, search results, the command `note <text>` / `notiz <text>` and a "Notes is not installed" state.
It does nothing, and logs one line, without Visor or with a Visor that has no module API 1.

### Install

```sh
scripts/pack-visor-module.sh                  # target/sheaf-visor@taulit.github.io.shell-extension.zip
gnome-extensions install --force target/sheaf-visor@taulit.github.io.shell-extension.zip
gnome-extensions enable sheaf-visor@taulit.github.io
```

Visor must be installed and enabled, and Sheaf (the app and `sheaf-searchd`) installed. Log out and in once after the
first install. German is the only translation so far; `scripts/update-visor-pot.sh` refreshes `visor-module/po/`.

### Unit tests

```sh
scripts/test-visor-module.sh
```

Plain `gjs`, no Node. The pure logic (result mapping, the note command, time labels, the view state) lives in
`visor-module/lib/` without shell imports and is tested in `visor-module/tests/*.test.js`. CI runs this and builds the zip.
There is no ESLint run: the config of Visor is for TypeScript and needs its Node toolchain.

### Check in a sandboxed shell

```sh
cargo build -p sheaf-searchd
scripts/visor-module-check.sh [--scheme dark|light] [--lang en|de] [--scenario main|absent|old]
```

Needs everything of the search check above plus `unzip` and a built Visor at the tag `api-v1`: the script builds it
from a local checkout of `taulit/visor` (`$VISOR_REPO`, default `~/development/visor`, with `node_modules`) into
`target/visor-api-v1/`, or takes `--visor DIR` with a built extension. The checkout is only read. A run takes about
40 seconds. It cannot run in CI, because Visor is a private repository.

`scripts/visor-sandbox.sh` is the sandbox of `shell-sandbox.sh` without the real app: Visor, the module (installed from
its zip), a real `sheaf-searchd` on a temporary notes folder, and a stand-in for the app
(`visor-module/tests/fake-sheaf-app.js`) that records what it is asked to do. The check asserts that

1. both extensions are active without error and the module registers;
2. the dock view lists the six newest notes with snippet and time, and Enter and Ctrl+Enter in its field create a
   note in the Quick Notes folder (Ctrl+Enter also asks the app to open it);
3. a click on a note, "Quick note" and "Open in Notes" reach the app;
4. the search finds a note, Sheaf's own GNOME provider is hidden, `note …` saves a quick note (and confirms it in the
   notch) and `notiz` alone opens the Quick Note window;
5. with the desktop file removed the view says "Notes is not installed" with a Flathub link, and it recovers when the
   file is back;
6. disabling and enabling the module, and Visor itself, leaves no actor behind and registers the module again;
7. with the service gone, the view says so and notes that cannot be saved are reported, not lost;
8. the shell log has no error about the module. `--scenario absent` and `--scenario old` check the one-line log for no
   Visor and a Visor without module API.

Screenshots of the dock view go to `docs/screenshots/visor-notes-{dark,light}.png`.

## Budgets

```sh
scripts/budgets.sh [RUNS]     # RUNS starts of the app, default 9
```

Builds the release binaries with `--locked` (`sheaf`, `sheaf-searchd`, the `search_budget` example and
`sheaf-budgets`, the measuring binary behind the `budgets` feature of `crates/sheaf`), generates notes with
`sheaf_core::testgen` (German and English, checklists, tables, tags; 15 % of the notes start with a
1600×1067 JPEG cover, hard links to eight generated photos), prints one table and exits non-zero when a
budget fails. A run takes about 4 minutes, most of it the two release builds.

Nothing of yours is touched: every measurement runs under `env -i` with its own `HOME` and XDG directories
in a temporary folder, the notes in `SHEAF_NOTES_DIR`, and a D-Bus session of its own whose only service
file is `sheaf-searchd`'s, so no portal or other desktop service starts. `LANG` is `en_US.UTF-8`.

| Budget (05 §1) | How it is measured | What the number leaves out |
|---|---|---|
| Cold start until the gallery is visible, 1,000 notes, < 250 ms | The real `sheaf` binary, started without a display and with `SHEAF_STARTUP_TRACE` (below). The index is built first by `sheaf-searchd --reindex`, as after a login; the app reads it and D-Bus-activates the service. Time from the spawn to the first painted frame in which every card shows its miniature; the first frame with cards is reported too. The budget is checked against the median of the runs after the first, with the binary, notes and index in the page cache (dropping the cache needs root). The first run, with a fresh XDG home, is reported apart. | The Wayland connection, the GPU device and surface, shader compilation and presenting: the headless platform has none. Its frame clock draws every 16.7 ms, so up to one frame of latency is in the number, as with a compositor. Covers still decoding do not delay the miniatures frame. |
| Memory with 1,000 notes, gallery open, < 120 MB RSS | `VmRSS` from `/proc/<pid>/status` and `Pss` from `smaps_rollup`, 2 s after the miniatures frame of each start; median. | GPU memory, mapped driver libraries and the compositor's buffers: the headless platform has no GPU surfaces, so the number on a desktop is higher. |
| Search p95, 10,000 notes, < 30 ms | `cargo run --release -p sheaf-core --features testgen --example search_budget`: an index of 10,000 notes on disk, the query mix of `testgen::QUERIES` (words, prefixes, typos, umlauts, phrases, filters), 25 rounds, 50 hits with snippets each. Also a CI job. | The D-Bus round trip of the shell or Visor. |
| Gallery scrolling, 60 fps | `sheaf-budgets scroll`: the gallery of 10,000 notes in a 1140×760 window at scale 1 on the headless platform, in process. 240 frames at 30 px, 240 at 150 px and 120 at 600 px a frame (reading, scrolling on, a fling), each a scroll event through the window and a full draw (layout, prepaint, paint into the scene); after each frame the rest of 16.7 ms passes, so miniatures and covers arrive from the background as they would. CPU time per frame, p50, p95 and max; the budget is p95 < 16.7 ms. | This is CPU frame time on the UI thread, not frames on screen: no GPU rendering, no upload of cover textures, no compositor. A real fps figure needs a display (manual checklist below). |
| `sheaf-searchd` idle memory, < 15 MB | `sheaf-budgets searchd`: the service on 1,000 and on 10,000 notes, `Sync()`, the query mix of `testgen::QUERIES` over D-Bus, then 10 s without a request (the service lets go of the index after 5 s); `VmRSS`, its anonymous part, and `Pss`. | Its short-lived `--index` child processes, which only live while they write. |

### The startup trace

`SHEAF_STARTUP_TRACE=<file>` makes the app append `<event> <µs since the Unix epoch>` for `run`
(`app::run` begins), `window` (the main window is open), `cards` (the first painted frame with cards,
followed by a line `card <path>` per note it shows) and `miniatures` (the first frame in which every card
drawn has its miniature), each once. Without `WAYLAND_DISPLAY` and `DISPLAY` the app then runs on the
headless platform and draws its windows at 60 Hz itself. Unset, it costs one look at a static per step.
`tests/trace.rs` checks it, `scripts/e2e.sh` uses it to see what the gallery shows.

### Not in CI

Only the search budget runs in CI (`search-budget` job): it needs no GPUI build and its margin is large. The
start, memory and scrolling budgets need a release build of GPUI and the editor (several minutes), and
CI cannot fetch the private editor repository at all at the moment (`docs/STATUS.md`). Frame and start
times on a shared runner also vary by more than the margins. They stay a local script; run it before a
release on a quiet machine.

## End-to-end scenarios

```sh
scripts/e2e.sh [--visor DIR]
```

Builds the workspace (debug), runs the scenarios of 05 §3 that involve Sheaf and can run without a visible
desktop, and prints pass or fail for each. Needs what the Visor module check needs (above), plus `sed`.

| 05 §3 | Coverage | By what |
|---|---|---|
| 5 Notiz anlegen | automated | `scripts/e2e.sh`: in the sandboxed headless shell with Visor (`api-v1`), the module and the real `sheaf-searchd`, `notiz Milch kaufen` in Visor's search; the notch confirms "Saved to Quick Notes"; the file is `Quick Notes/Milch kaufen.md`; the real app, started without a display, shows it in its first gallery frame (startup trace); the overview search finds "Milch kaufen" under the provider "Notes". |
| 6 Externe Änderung | automated, without a window on screen | `crates/sheaf/tests/e2e.rs` (run by the script): private bus, the real `sheaf-searchd` watching the folder, the app's Library in service mode, the note open on the real headless platform. `sed` checks an item: the note reloads, no banner. After typing, `sed` again: the banner "changed on disk" shows and the typed text stays. The in-process tests of `tests/note.rs` cover the banner's buttons (reload, keep mine). |
| 1–4, 7, 8 | not Sheaf's | Visor alone (notifications, timer, agent, calculator, module catalogue, switching off); tested in the Visor repository. |

What only a person at a real desktop can check, in a GNOME 50 session with Sheaf, `sheaf-searchd`, Visor
and the module installed:

1. **Notiz anlegen.** Open Visor's search, type `notiz Milch kaufen`, press Enter.
   Expected: the notch shows "In Schnellnotizen gespeichert" for about three seconds.
2. Start Notes from the app grid. Expected: the window appears within a quarter of a second; the gallery
   shows "Milch kaufen" under "Heute"; the chip "Schnellnotizen" lists it.
3. Press Super, type `Milch`. Expected: a group "Notizen" with "Milch kaufen"; Enter opens it in the app
   and the window comes to the front (activation token).
4. **Externe Änderung.** Create a note `Einkauf` with `- [ ] Milch` and `- [ ] Brot`, leave it open, run
   `sed -i 's/- \[ \] Milch/- [x] Milch/' ~/Dokumente/Notizen/Einkauf.md`. Expected: within a second the
   circle of "Milch" is filled, no banner, the caret stays in the note.
5. Type a word, then at once run `sed -i 's/Brot/Brötchen/'` on the file. Expected: the banner
   "Datei wurde extern geändert" with "Neu laden" and "Meine behalten"; "Neu laden" shows
   "Brötchen" without the word; "Meine behalten" keeps the word and writes a conflict copy next to it.
6. **Scrolling.** With 10,000 notes (`cargo run --release -p sheaf --features budgets --bin sheaf-budgets
   -- generate ~/Notizen-Test 10000 15`, then `sheaf --dir ~/Notizen-Test`), fling through the gallery
   with the touchpad. Expected: no visible stutter on a 60 Hz display; covers fade in without moving the
   cards. `GPUI` frame statistics or a screen recording at 60 fps show whether frames drop.
7. **Portals.** Export a note as PDF (the file chooser opens), print it (the print dialog opens), set the
   global Quick Note shortcut in Settings (the GNOME shortcut dialog opens). Expected: each dialog belongs
   to the Notes window and the result arrives (a PDF, a print job, a working shortcut).

## Coverage of plan 05 §2 "Sheaf"

| 05 §2 | Tests |
|---|---|
| `sheaf-core`: saving, renaming, collisions | `sheaf-core/tests/storage_store.rs` (names, ` 2` suffixes, long names, the file following the title) |
| Frontmatter minimal | `sheaf-core/src/frontmatter.rs`, `storage_store.rs` (byte-identical after a read/write cycle) |
| Trash with expiry | `sheaf-core/tests/storage_trash.rs` (30 days exactly, purge, attachments), `sheaf-searchd` empties it on start |
| External changes and conflicts | `storage_watch.rs`, sync conflict copies, `sheaf/tests/note.rs`, `tests/e2e.rs` |
| Index and search language: typos, umlauts, prefixes, filters, phrases, weighting | `sheaf-core/tests/search_query.rs`, `search_index.rs`, `src/search.rs`, `src/analysis.rs` |
| Benchmark with 10,000 generated notes | `sheaf-core/benches/search.rs`, `examples/search_budget.rs`, `scripts/budgets.sh` |
| D-Bus: all methods of both interfaces under a private bus | `sheaf-searchd/tests/service.rs` (`Search`, `Recent`, `Create`, `Append`, `Open`, `QuickNote`, `Sync`, `Reindex`, `NotesDir`, `Changed`; the five methods of `SearchProvider2`), the idle release of the index |
| UI with `TestAppContext`: gallery filters, navigation, Quick Note, saving on leave | `sheaf/tests/gallery.rs`, `app.rs`, `note.rs`, `quick.rs`, `shutdown.rs` (real headless platform) |
| Screenshots of the app, light and dark | `sheaf/tests/screenshots.rs` (`scripts/screenshot.sh`) |
| Visor module: the shell scenarios | `scripts/visor-module-check.sh`, `scripts/e2e.sh` |

Left open:

- The app's screenshots are rendered offscreen by `TestAppContext`, not in the headless shell as 05 §2 has
  it: a GPUI window in the headless shell needs a GPU path the sandbox does not promise.
- The Visor module check talks to a stand-in for the app (`fake-sheaf-app.js`), not the debug build of the
  app; only scenario 5 runs the real app, without a display.
- The card's context menu is not opened by any test (its actions are called directly): GPUI's test context
  has no way to find the card's bounds without debug selectors.

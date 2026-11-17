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

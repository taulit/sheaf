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

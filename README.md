# Sheaf

Fast, native notes for GNOME – Apple Notes–style, plain Markdown files.

Sheaf is in early development. The plan is in [docs/design/plan](docs/design/plan/README.md),
the current state in [docs/STATUS.md](docs/STATUS.md), and the cross-crate decisions in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo run -p sheaf            # the app; `cargo run -p sheaf -- help` for the commands
scripts/screenshot.sh         # renders docs/screenshots/app-*.png offscreen
```

Requires:

- Rust 1.95 or newer.
- Access to [gpui-markdown-editor](https://github.com/taulit/gpui-markdown-editor), a
  private repository for now. Cargo fetches it over https with your git credentials;
  `CARGO_NET_GIT_FETCH_WITH_CLI=true` lets it use the git command line and its ssh setup.
- The libraries GPUI builds against. On Debian and Ubuntu:
  `pkg-config libfontconfig-dev libfreetype-dev libwayland-dev libxkbcommon-dev
  libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev`. Running the app needs a
  Vulkan driver (Mesa has one for every common GPU).
- For the tests, `dbus-daemon` and `gettext`; for the screenshots, a GPU or Mesa's
  software Vulkan driver (lavapipe).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

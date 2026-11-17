# Sheaf

Fast, native notes for GNOME – Apple Notes–style, plain Markdown files.

Sheaf is in early development. The plan is in [docs/design/plan](docs/design/plan/README.md),
the current state in [docs/STATUS.md](docs/STATUS.md), and the cross-crate decisions in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build

```sh
cargo build --workspace
cargo test --workspace
```

Requires Rust 1.91 or newer and, for the tests, `dbus-daemon`.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

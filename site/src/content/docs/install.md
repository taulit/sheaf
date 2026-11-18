---
title: Install Sheaf on Linux
description: How to install Sheaf, the native GNOME notes app, from Flatpak, the Arch User Repository or from source, and what each install needs.
---

Sheaf 0.1.0 is the first release. It runs on GNOME (version 50 is what it is tested with)
and on other Wayland or X11 desktops, but the overview search and the Quick Note shortcut
are GNOME features.

## What is available

| Way | Status |
|---|---|
| From source | Works today. |
| Flatpak (Flathub) | Manifest is in the repository, submission to Flathub is pending. |
| Arch Linux (AUR) | `PKGBUILD`s for `sheaf` and `sheaf-git` are in the repository, not yet in the AUR. |

This page will name the Flathub and AUR packages as soon as they are live.

## Flatpak

Once Sheaf is on Flathub:

```sh
flatpak install flathub io.github.taulit.Sheaf
```

The Flatpak can read your Documents folder. For a notes folder elsewhere, give it access:

```sh
flatpak override --user --filesystem=~/Sync/Notes io.github.taulit.Sheaf
```

Flatpak turns exported search providers off. To find notes in the GNOME overview, switch
Sheaf on once: **Settings → Search →** the switch next to **Notes**.

## Arch Linux (AUR)

Once the packages are in the AUR:

```sh
yay -S sheaf        # the release
yay -S sheaf-git    # the development version
```

## From source

You need Rust 1.95 or newer and the libraries GPUI builds against.

- **Debian and Ubuntu:** `pkg-config libfontconfig-dev libfreetype-dev libwayland-dev
  libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libvulkan-dev`
- **Arch:** `pkgconf fontconfig freetype2 wayland libxkbcommon libxkbcommon-x11 libxcb
  vulkan-icd-loader`

Running the app needs a Vulkan driver. Mesa has one for every common GPU.

```sh
git clone https://github.com/taulit/sheaf && cd sheaf
cargo build --release --locked
cargo build --profile service --locked -p sheaf-searchd
sudo build-aux/install.sh /usr/local
```

`install.sh` puts the binaries, desktop file, icons, metainfo, D-Bus services, search
provider, translations and shell completions in place. Use `/usr` or `/usr/local`: GNOME
Shell reads search providers only from the system data directories.

The session's D-Bus does not see new service files until it reloads. Log out and in once,
or run:

```sh
busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
```

## Uninstall

Remove the files `install.sh` listed, or uninstall the package. Your notes stay where they
are: they are your Markdown files, Sheaf never moves them into its own storage.

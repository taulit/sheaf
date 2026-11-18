---
title: FAQ
description: Frequently asked questions about Sheaf - why another notes app, how it compares to Obsidian and GNOME Notes, sync, mobile, Flatpak and privacy.
---

## Why another notes app?

I wanted something like Apple Notes on GNOME: open it, see all notes as cards, type, find
anything instantly. But with the notes as plain files, not in a database. Sheaf is that.

## How is it different from Obsidian or Logseq?

Sheaf is smaller and native. It does not try to be a knowledge base with plugins and graphs.
It starts in well under a second, fits into GNOME (overview search, portals, a Quick Note
shortcut) and keeps the same Markdown files those tools can open too. Many people can use
both on the same folder.

## How is it different from GNOME's own notes apps?

GNOME Notes (Bijiben) stores notes in its own format. Sheaf stores Markdown files you can use
anywhere, and adds a gallery, tags, smart folders, typo-tolerant search and a command line.

## Is it built with GTK?

No. Sheaf uses [GPUI](https://www.gpui.rs), the GPU-accelerated Rust UI framework from the
Zed editor, with a look modelled on GNOME and libadwaita. The editor is
[gpui-markdown-editor](https://taulit.github.io/gpui-markdown-editor/), a separate
component.

## How do I sync my notes?

With any tool that syncs folders: Syncthing, Nextcloud, Dropbox, git. Sheaf notices changes
from outside while it runs and never overwrites your changes silently.

## Is there a mobile app?

No. The files work in any Markdown app on your phone.

## Does Sheaf send data anywhere?

No. It has no network access, no telemetry and no account. The Flatpak has no network
permission at all.

## Which desktops does it run on?

It is made for GNOME and tested on GNOME 50. It runs on other Wayland and X11 desktops,
without the overview search.

## How fast is it really?

Measured on the release build: about 6 ms per search (95th percentile) with 10,000 notes,
and about 170 ms from start to a full gallery with 1,000 notes. The numbers and how they are
measured are in
[docs/STATUS.md](https://github.com/taulit/sheaf/blob/main/docs/STATUS.md).

## How can I help?

Try it and open an issue with what is missing or broken:
[github.com/taulit/sheaf/issues](https://github.com/taulit/sheaf/issues). Reviews of the
machine-assisted translations by native speakers are very welcome.

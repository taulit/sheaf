# Assets

Everything in here is compiled into the binary by `src/app/assets.rs`.

## `icons/`

Symbolic icons of the GNOME Adwaita icon theme, copied unchanged from
`/usr/share/icons/Adwaita/symbolic/` of `adwaita-icon-theme` 50.0
(<https://gitlab.gnome.org/GNOME/adwaita-icon-theme>). Licence: CC-BY-SA-3.0 or
LGPL-3.0-only, at your choice; © the GNOME Project. They are drawn as masks in the
text colour, so their own fill colour does not matter.

| File | Adwaita name |
|---|---|
| `dialog-warning.svg` | `status/dialog-warning-symbolic` |
| `document-edit.svg` | `actions/document-edit-symbolic` |
| `document-print.svg` | `actions/document-print-symbolic` |
| `document-save-as.svg` | `actions/document-save-as-symbolic` |
| `edit-clear.svg` | `actions/edit-clear-symbolic` |
| `edit-copy.svg` | `actions/edit-copy-symbolic` |
| `edit-delete.svg` | `actions/edit-delete-symbolic` |
| `edit-find.svg` | `actions/edit-find-symbolic` |
| `edit-undo.svg` | `actions/edit-undo-symbolic` |
| `folder.svg` | `places/folder-symbolic` |
| `folder-new.svg` | `actions/folder-new-symbolic` |
| `folder-saved-search.svg` | `places/folder-saved-search-symbolic` |
| `go-next.svg` | `actions/go-next-symbolic` |
| `go-previous.svg` | `actions/go-previous-symbolic` |
| `image-x-generic.svg` | `mimetypes/image-x-generic-symbolic` |
| `list-add.svg` | `actions/list-add-symbolic` |
| `mail-attachment.svg` | `status/mail-attachment-symbolic` |
| `object-select.svg` | `actions/object-select-symbolic` |
| `open-menu.svg` | `actions/open-menu-symbolic` |
| `pan-down.svg` | `ui/pan-down-symbolic` |
| `send-to.svg` | `actions/send-to-symbolic` |
| `sidebar-show.svg` | `actions/sidebar-show-symbolic` |
| `text-x-generic.svg` | `mimetypes/text-x-generic-symbolic` |
| `user-trash.svg` | `places/user-trash-symbolic` |
| `view-grid.svg` | `actions/view-grid-symbolic` |
| `view-list.svg` | `actions/view-list-symbolic` |
| `view-more.svg` | `actions/view-more-symbolic` |
| `view-pin.svg` | `actions/view-pin-symbolic` |
| `view-sort-descending.svg` | `actions/view-sort-descending-symbolic` |
| `window-close.svg` | `ui/window-close-symbolic` |

To add one: copy `<name>-symbolic.svg` here as `<name>.svg`, add a row above and a
variant to `IconName` in `src/app/assets.rs`.

## `fonts/`

Inter 4.1 by The Inter Project Authors (<https://github.com/rsms/inter>), the static
TTFs `extras/ttf/Inter-{Regular,Medium,SemiBold,Bold,ExtraBold}.ttf` of the release
archive `Inter-4.1.zip`, copied unchanged. Licence: SIL Open Font License 1.1, see
`fonts/OFL.txt`. Adwaita Sans, GNOME's interface font, is derived from Inter and has
the same metrics.

They are there because GPUI 0.3.12 does not pick weights from a variable font: with
the variable Adwaita Sans or Inter that distributions install, bold text would come
out regular. `theme.rs` registers them when the desktop font has no face of its own
for bold, and draws with Inter then.

## App icon

Not copied: `assets.rs` embeds `data/icons/hicolor/` directly, as `icons/app.svg` and
`icons/app-symbolic.svg`. See `data/icons/README.md` for its origin.

---
title: Using Sheaf
description: How to work with notes in Sheaf - the gallery, folders and tags, editing in place, checklists, tables, links between notes, Quick Note and Recently Deleted.
---

## The gallery

Sheaf opens on a gallery of cards. Each card shows the first lines of a note, its title and
when it changed. Cards are grouped into **Pinned**, **Today**, **Yesterday**, the last days
and older months. The buttons in the header switch between the gallery and a list, change
the sort order and create a note.

Above the cards are chips for your folders, tags and smart folders. A click shows only those
notes. The sidebar (the button on the left of the header) shows the same as a tree.

## Writing

Press **Ctrl+N** or the pencil button. The first line becomes the title, and the file name
follows it. There is no save button: a note is written 0.6 seconds after the last change and
at once when you go back to the gallery, close the window or quit.

The editor renders Markdown as you type:

- `# ` starts a heading, `- ` a list, `[ ] ` a checklist item, `> ` a quote.
- **Checklists**: click the circle to tick an item.
- **Tables**: from the format bar or the `/` menu.
- **Images and files**: paste or drop them in. They go to `_attachments/` next to the note.
- **Links between notes**: type `[[` and pick a note. A note shows which notes link to it.
- **Tags**: write `#word` anywhere. Nested tags like `#work/launch` work too.

Type `/` at the start of a line for the block menu, or `/template` to insert one of your
templates.

## Folders, pins and smart folders

- Folders are subfolders of your notes folder. Create, rename and delete them from the
  sidebar or the folder chips.
- **Ctrl+P** pins a note to the top of the gallery.
- A **smart folder** is a saved search, for example `#work open:` for every work note with
  unchecked tasks.

## Quick Note

**Ctrl+Shift+N** opens a small window for a quick thought, also when Sheaf is not in front.
Quick notes land in the folder **Quick Notes**. Set the global shortcut under **Settings →
Shortcut from Anywhere**.

## Finding notes from GNOME

Press Super and type. Notes that match show up under **Notes** in the overview; Enter opens
the note in Sheaf.

## Recently Deleted

Deleting a note moves it to **Recently Deleted** (the `.trash/` folder). It is removed for
good after 30 days. Until then you can restore it.

## Export and print

The share button of a note exports it as Markdown or PDF, or prints it.

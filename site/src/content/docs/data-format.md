---
title: Your notes folder – plain Markdown files
description: How Sheaf stores notes - one Markdown file per note, subfolders as folders, tags in the text, minimal frontmatter, attachments next to the notes. Sync with any tool.
---

Sheaf has no database of its own. Your notes are ordinary files that you can read, edit,
back up and sync with whatever you already use.

## Layout

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../screenshots/files-dark.webp" />
  <img src="../screenshots/files-light.webp" alt="A terminal next to Sheaf listing the notes folder: one Markdown file per note, subfolders as folders" width="1600" height="1027" loading="lazy" />
</picture>

```text
~/Documents/Notes/
├── Groceries.md
├── Work/
│   ├── Launch plan.md
│   └── Meeting notes – design review.md
├── Recipes/
│   └── Banana bread.md
├── _attachments/        images and files, linked with relative paths
├── .trash/              Recently Deleted, emptied after 30 days
└── .sheaf/              settings and templates of this folder
```

- **One `.md` file per note.** The first line is the title; the file name follows it when
  the title changes.
- **Subfolders are the folders** of the app.
- **Tags are `#words` in the text.** No hidden metadata.
- **Attachments** live in `_attachments/` and are linked with relative paths, so other
  Markdown editors show them too.

## A note on disk

```markdown
---
pinned: true
---
Launch plan

Ship 0.1.0 on Friday.

- [ ] Finish the Flatpak manifest
- [x] Write the release notes

#launch
```

Frontmatter is only written when it is needed: an `id` that survives renames, `created`,
and `pinned`. A file that Sheaf did not change stays byte for byte the same, and its
frontmatter keys and comments are kept.

## Where things are

| What | Where |
|---|---|
| Notes | `~/Documents/Notes` by default (`~/Documents/Notizen` in German) |
| Choose another folder | Settings, the `SHEAF_NOTES_DIR` variable, or `--dir` |
| Search index | `$XDG_CACHE_HOME/sheaf`, safe to delete, it is rebuilt |

## Sync and other editors

Put the folder in Syncthing, Nextcloud, Dropbox, a git repository or anything else. Changes
from outside show up while Sheaf runs. If a note changes on disk while you type in it, Sheaf
asks whether to reload it or keep your version, and keeps a conflict copy.

The folder works with other Markdown tools such as Obsidian, Zettlr or a plain text editor.
Links between notes use the `[[Title]]` form.

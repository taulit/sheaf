# Sheaf launch posts (drafts)

Drafts for Reddit, posted by the developer from their personal account. Nothing here has
been posted. Each post is meant to be posted once, by hand, after the checks below.

## Before posting

These have to be true first, or the first comment will be "I can't build it":

1. `taulit/sheaf` is public.
2. `taulit/gpui-markdown-editor` is public too. Sheaf depends on it as a git dependency, so a
   build from source fails while it is private.
3. The `v0.1.0` release is published (it is a draft now) and its screenshot URLs resolve.
4. The docs site is live: enable GitHub Pages (Settings → Pages → Source: GitHub Actions),
   then run the "Docs site" workflow or merge to `main`. URL: <https://taulit.github.io/sheaf/>.
5. Fill in the links in the drafts below if any of them change.

Optional but helpful: Flathub or AUR packages. Without them the only install path is from
source, which the posts say plainly.

## How to post

- Upload `marketing/video/out/sheaf-demo.mp4` as a **native Reddit video** (not a YouTube
  link). It autoplays muted in the feed, which is why the video has captions and no sound.
- Put the text in the post body, links included. Add the repo link as the first comment
  too, because some apps hide the body of video posts.
- One subreddit per day at most, not all at once. Start with r/gnome.
- Stay around for the first two hours and answer every comment. Bug reports become issues.
- Do not cross-post the same text everywhere; the drafts below differ on purpose.

## Subreddits and their rules

**Not verified live.** Reddit blocks automated access from this machine (plain requests, a
headless browser, and a logged-in headless browser all got "blocked by network security"),
and web searches only turned up third-party summaries that contradict each other. The notes
below are from what I know of these communities and must be checked against each sidebar
(old.reddit.com/r/NAME/about/rules) before posting.

| Subreddit | Fit | What to check before posting |
|---|---|---|
| r/gnome (~140k) | Best fit: a GNOME app with overview search and Quick Note. App showcases are common there. | Sidebar rules and required flair (look for an "Apps"/"Project" flair). Whether non-GTK apps are welcome: Sheaf uses GPUI, not GTK/libadwaita, say so up front. |
| r/rust | Good fit for the technical angle (GPUI, tantivy, budgets). Project posts are normal there when they are about Rust. | The current rule on AI-generated code and posts. Recent moderation has been strict about undisclosed LLM-written projects; disclose (see below). Use the technical draft, not the GNOME one. |
| r/linux | Large and critical. Project announcements are allowed if relevant to Linux, but low-effort self-promotion gets removed. | Self-promotion rule and whether a weekly thread is required for new software. Post only after r/gnome went well. |
| r/linuxapps | Small, made for exactly this kind of post. Low risk. | Flair. |
| r/opensource | Fine for "I made a GPL app" posts. | Self-promotion rule; some periods require a "Promotional" flair. |
| r/ObsidianMD | Interesting audience (plain Markdown vault), but promoting another app there is often unwelcome. | Read rules carefully; skip unless there is a "showcase"/"other apps" allowance. |
| r/selfhosted | **Skip.** Sheaf is a desktop app, not a self-hosted service; it is off-topic there. | – |

General Reddit norm: keep self-promotion a small part of your activity on that account and
comment in those communities before and after posting.

### About AI assistance (decide before posting)

Most of Sheaf was written with AI coding agents: about 370 of the commits carry a
`Co-Authored-By: Claude` line, and the whole history spans a few days. Anyone who opens
the repository will see that within a minute. Several of the subreddits above (r/rust above
all) react badly to undisclosed AI-built projects and some remove them. The drafts therefore
say it openly in one sentence. Adjust the wording to how you see your own role (design,
specs, review, testing on real hardware), but do not remove it.

---

## r/gnome

**Title:** I built a native notes app for GNOME that keeps every note as a plain Markdown file

**Flair:** whatever the subreddit uses for apps/projects

**Body:**

> I wanted Apple Notes on my GNOME desktop: open it, see every note as a card, type, find
> anything instantly. But I also wanted my notes as plain files I can sync and open
> anywhere, not in a database. I couldn't find that, so I built **Sheaf** (it shows up as
> "Notes" in GNOME).
>
> What it does:
>
> - Gallery of cards with a miniature of each note, grouped by date; folders, tags, pinned
>   notes, smart folders, a list view
> - Edit in place: checklists, tables, images, `[[links]]` between notes, a slash menu
> - Search that forgives typos and knows `#tag`, `folder:`, `has:checklist`, `open:`. About
>   6 ms with 10,000 notes
> - Notes in the GNOME overview search, and a Quick Note window on Ctrl+Shift+N
> - Every note is a `.md` file in a folder you choose; subfolders are folders. Sync with
>   Syncthing, Nextcloud, git, whatever you use
>
> Honest notes:
>
> - It is **not GTK**. It is written in Rust with GPUI (the UI framework of the Zed editor),
>   styled to look like a libadwaita app. It follows the dark/light setting and uses portals,
>   but it will not pick up GTK themes.
> - It is 0.1.0. For now it installs from source; a Flatpak manifest and AUR PKGBUILDs are in
>   the repo but not on Flathub/AUR yet.
> - I built it with heavy help from AI coding agents. I designed it, wrote the specs, and
>   reviewed and tested it on my own desktop, but most of the code was written by the agents.
> - GPL-3.0, no network access, no telemetry, no account.
>
> I'd really like feedback from GNOME users: what feels off compared to a "real" GNOME app,
> what you'd miss coming from GNOME Notes, Apple Notes or Obsidian, and whether the
> plain-Markdown approach works with how you sync.
>
> Code: https://github.com/taulit/sheaf
> Docs: https://taulit.github.io/sheaf/

---

## r/rust

**Title:** I built a GNOME notes app in Rust with GPUI: plain Markdown files, tantivy search, 6 ms p95 on 10k notes

**Flair:** "🛠️ project" (or whatever the current project flair is)

**Body:**

> **Sheaf** is a notes app for GNOME: a gallery of note cards, WYSIWYG editing, and every
> note is a plain `.md` file. I wanted to share how it is put together and get feedback on
> the architecture.
>
> **Stack**
>
> - UI: [GPUI](https://www.gpui.rs) (Zed's framework) + gpui-component. The Markdown editor
>   is a separate crate, [gpui-markdown-editor](https://github.com/taulit/gpui-markdown-editor),
>   that round-trips Markdown without losing formatting.
> - `sheaf-core`: the notes folder (atomic writes, renames that follow the title, a trash,
>   a `notify` watcher), the search language, and a **tantivy** index. Frontmatter is edited
>   key by key, so a file Sheaf did not change stays byte-identical.
> - `sheaf-searchd`: a D-Bus service (zbus) that is the only writer of the index. It also
>   implements GNOME Shell's `SearchProvider2`, so notes appear in the overview. Index writes
>   run in a short-lived child process because tantivy's writer leaves ~7 MB behind.
> - The same `sheaf` binary is a CLI with stable JSON output for scripts and agents.
> - PDF export with krilla and its own line breaking.
>
> **Budgets** (release build, measured, numbers in `docs/STATUS.md`):
>
> - search p95 with 10,000 notes: ~6 ms (budget 30 ms)
> - cold start to a full gallery with 1,000 notes: ~170 ms
> - gallery scrolling with 10,000 notes: CPU frame p95 10.7 ms
>
> **Disclosure:** most of the ~45k lines were written by AI coding agents (Claude) from my
> specs, with me reviewing and testing; the commit history shows it. There are ~12k lines of
> tests, property tests for the editor round-trip, and end-to-end checks that run the app in
> a headless GNOME Shell.
>
> Questions I'd love input on: GPUI for a desktop app outside Zed (what bit you?), the
> one-writer index process, and whether the CLI JSON schema makes sense.
>
> Repo (GPL-3.0): https://github.com/taulit/sheaf
> Docs: https://taulit.github.io/sheaf/

---

## r/linux

**Title:** Sheaf: a fast native notes app that stores everything as plain Markdown files (GNOME, Rust, GPL)

**Body:**

> I built a notes app for the Linux desktop with one rule: your notes are ordinary Markdown
> files in a folder you choose, never a database. On top of that it tries to be as quick
> and pleasant as Apple Notes: a gallery of cards, editing in place, and search in a few
> milliseconds, also with 10,000 notes.
>
> It is made for GNOME (overview search, Quick Note shortcut, portals) but runs on other
> Wayland and X11 desktops. It is written in Rust with GPUI, not GTK. Version 0.1.0, installs
> from source for now; Flatpak and AUR are prepared. Built with a lot of help from AI coding
> agents, which I want to be upfront about. No network access, no telemetry.
>
> I'm mostly looking for feedback: does the plain-files approach work with your sync setup,
> and what would stop you from switching from what you use now?
>
> https://github.com/taulit/sheaf · https://taulit.github.io/sheaf/

---

## r/linuxapps and r/opensource

Use the r/linux text with the title:

**Title:** I made Sheaf, an open-source notes app for GNOME where every note is a plain Markdown file

---

## First comment (all subreddits)

> Repo: https://github.com/taulit/sheaf — docs and install: https://taulit.github.io/sheaf/
> Happy to answer questions about GPUI, the search, or the file format.

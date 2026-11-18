# Marketing

Material for making Sheaf known. Nothing in here is published automatically.

| What | Where |
|---|---|
| Reddit drafts, subreddit notes, checklist before posting | [posts.md](posts.md) |
| Demo video (Remotion project) | [video/](video/) |
| Docs site (Astro Starlight, GitHub Pages) | [../site/](../site/) and [../.github/workflows/pages.yml](../.github/workflows/pages.yml) |

## Demo video

The footage is a real screen recording of the release build. It runs in the sandboxed
headless GNOME Shell of [scripts/shell-sandbox.sh](../scripts/shell-sandbox.sh) on a
2880x1620 virtual monitor at scale 2, so it never touches your desktop. A virtual mouse
and keyboard ([video/record/input.js](video/record/input.js)) open a note, tick an item,
type, search and show the files in GNOME Console. The notes are
[video/demo-notes/](video/demo-notes/).

```sh
cargo build --release --workspace
cd marketing/video
pnpm install
pnpm record                  # public/recording-light.mp4 + scene times (record/record.sh dark for dark)
pnpm render                  # out/sheaf-demo.mp4, 1920x1080, ~43 s, no sound
pnpm render:square           # out/sheaf-demo-square.mp4, 1080x1080
pnpm gif                     # docs/screenshots/sheaf-demo.gif for the README
pnpm og                      # site/public/og.png
pnpm studio                  # edit cuts, zooms and captions in the browser
```

Recording needs GNOME Shell 50, PipeWire and WirePlumber, GNOME Console and ffmpeg;
ImageMagick only for the site screenshots below. Captions, cuts and
camera moves are in [video/src/Demo.tsx](video/src/Demo.tsx); they follow the scene times
in `public/recording-light.json`, so a new recording needs no edits there.

Site screenshots come from the stills the recording writes:

```sh
for n in gallery edit search files; do for s in light dark; do
  magick marketing/video/public/stills/$n-$s.png -crop 2400x1540+240+80 +repage \
    -resize 1600x -strip -quality 90 site/public/screenshots/$n-$s.webp
done; done
cp marketing/video/out/sheaf-demo.mp4 site/public/media/
```

## Docs site

```sh
cd site
pnpm install
pnpm dev        # http://localhost:4321/sheaf/
pnpm build      # dist/, with sitemap and search index
```

The workflow "Docs site" builds it on pull requests and deploys it from `main`. It needs
GitHub Pages switched on once: Settings → Pages → Source: GitHub Actions.

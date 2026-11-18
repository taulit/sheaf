//! One pass over the Markdown body that collects everything Sheaf derives from the
//! text: title, headings, tags, checklist items, wiki links and content flags.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

/// What [`scan`] found in a note body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Text of the first non-empty line without Markdown markup; empty for an empty note.
    pub title: String,
    /// All headings except a heading on the title line.
    pub headings: Vec<String>,
    /// `#tags` without the `#`, first spelling of each.
    pub tags: Vec<String>,
    pub tasks: Vec<Task>,
    /// Targets of `[[wiki links]]` without label and heading.
    pub links: Vec<String>,
    pub has_image: bool,
    pub has_table: bool,
    /// The note links to a relative file that is neither an image nor a note.
    pub has_attachment: bool,
}

/// A checklist item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Position among the tasks of the note, starting at 1.
    pub number: u32,
    pub checked: bool,
    pub text: String,
    /// Bytes of the `[ ]` or `[x]` marker within the body.
    pub marker: Range<usize>,
}

const IMAGE_EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp", "heic", "tif", "tiff",
];

/// Characters after which a `#` may start a tag, besides whitespace.
const OPENING_PUNCTUATION: [char; 10] = ['(', '[', '{', '"', '\'', '„', '“', '‚', '‘', '«'];

/// Scans a note body (the file text without frontmatter).
pub fn scan(body: &str) -> Scan {
    let mut scanner = Scanner {
        body,
        title_line: first_line(body),
        scan: Scan::default(),
        heading: None,
        task_open: false,
        in_code_block: false,
        in_bare_wikilink: false,
    };
    for (event, range) in Parser::new_ext(body, options()).into_offset_iter() {
        scanner.event(event, range);
    }
    scanner.finish()
}

/// The files a note body links to or shows, as the app opens them: percent escapes
/// decoded, without `?query`, `#fragment` and leading `./`. Web addresses, anchors,
/// wiki links and anything inside code are left out.
pub fn file_targets(body: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for event in Parser::new_ext(body, options()) {
        let (Event::Start(Tag::Link {
            link_type,
            dest_url,
            ..
        })
        | Event::Start(Tag::Image {
            link_type,
            dest_url,
            ..
        })) = event
        else {
            continue;
        };
        if matches!(link_type, LinkType::WikiLink { .. } | LinkType::Email) || has_scheme(&dest_url)
        {
            continue;
        }
        let path = dest_url.split(['?', '#']).next().unwrap_or_default();
        let mut path = percent_decode(path);
        while let Some(rest) = path.strip_prefix("./") {
            path = rest.to_owned();
        }
        if !path.is_empty() {
            targets.push(path);
        }
    }
    targets
}

/// `%20` and friends in a link target, as files on disk do not have them.
pub fn percent_decode(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let [first, tail @ ..] = rest {
        let escaped = match (first, tail) {
            (b'%', [high, low, ..]) => std::str::from_utf8(&[*high, *low])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok()),
            _ => None,
        };
        match escaped {
            Some(byte) => {
                bytes.push(byte);
                rest = &tail[2..];
            }
            None => {
                bytes.push(*first);
                rest = tail;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Frontmatter tags followed by text tags, without repeats that differ only in case.
pub fn merge_tags(frontmatter: &[String], text: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    for tag in frontmatter.iter().chain(text) {
        push_unique(&mut merged, tag);
    }
    merged
}

/// Whether `text` is a tag as it may follow a `#`.
pub fn is_tag(text: &str) -> bool {
    tag_len(text) == text.len() && !text.is_empty() && !text.ends_with(['/', '-'])
}

struct Scanner<'a> {
    body: &'a str,
    /// Bytes of the first non-empty line.
    title_line: Range<usize>,
    scan: Scan,
    /// Start offset and text of the heading being read.
    heading: Option<(usize, String)>,
    /// The last task still collects its text.
    task_open: bool,
    in_code_block: bool,
    /// Inside `[[Target]]`, whose text repeats the target and may hold `#heading`.
    in_bare_wikilink: bool,
}

impl Scanner<'_> {
    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(end) => self.end(end, range),
            Event::Text(text) => {
                if !self.in_code_block && !self.in_bare_wikilink {
                    self.tags_in(range.clone());
                }
                self.text(&text, range.start);
            }
            Event::Code(text) => self.text(&text, range.start),
            Event::SoftBreak | Event::HardBreak => self.text(" ", usize::MAX),
            Event::TaskListMarker(checked) => {
                self.scan.tasks.push(Task {
                    number: self.scan.tasks.len() as u32 + 1,
                    checked,
                    text: String::new(),
                    marker: range,
                });
                self.task_open = true;
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Heading { .. } => self.heading = Some((range.start, String::new())),
            Tag::CodeBlock(_) => self.in_code_block = true,
            Tag::List(_) => self.task_open = false,
            Tag::Table(_) => self.scan.has_table = true,
            Tag::Link {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            } => {
                let target = dest_url.split(['#', '|']).next().unwrap_or_default().trim();
                if !target.is_empty() && !self.scan.links.iter().any(|link| link == target) {
                    self.scan.links.push(target.to_owned());
                }
                self.in_bare_wikilink = !has_pothole;
            }
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let typed = !matches!(link_type, LinkType::Autolink | LinkType::Email);
                if typed && target_kind(&dest_url) == TargetKind::File {
                    self.scan.has_attachment = true;
                }
            }
            Tag::Image {
                link_type,
                dest_url,
                ..
            } => match target_kind(&dest_url) {
                TargetKind::File => self.scan.has_attachment = true,
                TargetKind::Note => {}
                // `![[Note]]` embeds a note, not a picture.
                TargetKind::Other if matches!(link_type, LinkType::WikiLink { .. }) => {}
                TargetKind::Image | TargetKind::Other => self.scan.has_image = true,
            },
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd, range: Range<usize>) {
        match end {
            TagEnd::Heading(_) => {
                if let Some((start, text)) = self.heading.take()
                    && !self.title_line.contains(&start)
                {
                    self.scan.headings.push(text.trim().to_owned());
                }
            }
            TagEnd::CodeBlock => self.in_code_block = false,
            TagEnd::Item | TagEnd::Paragraph => self.task_open = false,
            TagEnd::Link => self.in_bare_wikilink = false,
            TagEnd::TableCell if self.title_line.contains(&range.start) => {
                self.scan.title.push(' ');
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str, start: usize) {
        if let Some((_, heading)) = &mut self.heading {
            heading.push_str(text);
        }
        if self.task_open
            && let Some(task) = self.scan.tasks.last_mut()
        {
            task.text.push_str(text);
        }
        if self.title_line.contains(&start) {
            self.scan
                .title
                .push_str(text.lines().next().unwrap_or_default());
        }
    }

    fn tags_in(&mut self, range: Range<usize>) {
        let text = &self.body[range.clone()];
        for (at, _) in text.match_indices('#') {
            let before = self.body[..range.start + at].chars().next_back();
            let opens =
                before.is_none_or(|c| c.is_whitespace() || OPENING_PUNCTUATION.contains(&c));
            let rest = &text[at + 1..];
            let tag = rest[..tag_len(rest)].trim_end_matches(['/', '-']);
            if opens && !tag.is_empty() {
                push_unique(&mut self.scan.tags, tag);
            }
        }
    }

    fn finish(mut self) -> Scan {
        for task in &mut self.scan.tasks {
            task.text = task.text.trim().to_owned();
        }
        let title = self.scan.title.trim();
        self.scan.title = if title.is_empty() {
            // Nothing but markup on the line (a rule, a fence, an empty heading).
            let line = self.body[self.title_line].trim();
            let unmarked = line.trim_start_matches('#');
            if unmarked.is_empty() || unmarked.starts_with(' ') {
                unmarked.trim().to_owned()
            } else {
                line.to_owned()
            }
        } else {
            title.to_owned()
        };
        self.scan
    }
}

/// Bytes of the first line that is not blank, without its line ending.
fn first_line(body: &str) -> Range<usize> {
    let mut start = 0;
    for line in body.split_inclusive('\n') {
        if !line.trim().is_empty() {
            return start..start + line.trim_end_matches(['\r', '\n']).len();
        }
        start += line.len();
    }
    0..0
}

/// Length of the tag at the start of `text`: a letter, then letters, digits, `_`, `-`, `/`.
fn tag_len(text: &str) -> usize {
    if !text.starts_with(char::is_alphabetic) {
        return 0;
    }
    text.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')))
        .unwrap_or(text.len())
}

fn push_unique(tags: &mut Vec<String>, tag: &str) {
    let lower = tag.to_lowercase();
    if !tags.iter().any(|known| known.to_lowercase() == lower) {
        tags.push(tag.to_owned());
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TargetKind {
    /// A relative path to an image file.
    Image,
    /// A relative path to another note.
    Note,
    /// A relative path to any other file: an attachment.
    File,
    /// A URL, an absolute path, an anchor, or a path without extension.
    Other,
}

fn options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_WIKILINKS);
    options
}

/// Whether `target` starts with a URL scheme such as `https:` or `mailto:`.
fn has_scheme(target: &str) -> bool {
    let scheme_end = target.find(':').unwrap_or(target.len());
    scheme_end < target.len()
        && target[..scheme_end]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn target_kind(target: &str) -> TargetKind {
    if has_scheme(target) || target.starts_with(['/', '#']) {
        return TargetKind::Other;
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    let name = path.rsplit('/').next().unwrap_or_default();
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => {
            let extension = extension.to_ascii_lowercase();
            if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                TargetKind::Image
            } else if extension == "md" {
                TargetKind::Note
            } else {
                TargetKind::File
            }
        }
        _ => TargetKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_the_first_line_without_markup() {
        let cases = [
            ("", ""),
            ("\n\n  \n", ""),
            ("Einkauf\nMilch", "Einkauf"),
            ("\n\n# Heading title\n\nText", "Heading title"),
            (
                "## **Bold** and `code` and [link](https://example.com)",
                "Bold and code and link",
            ),
            ("Title ##\n", "Title ##"),
            ("# Closed heading #\n", "Closed heading"),
            ("Setext\n======\n", "Setext"),
            ("- [ ] Milk\n- [ ] Eggs\n", "Milk"),
            ("> Quoted line\n", "Quoted line"),
            ("#tag starts the note\n", "#tag starts the note"),
            ("[[Other note|label]] first\n", "label first"),
            ("Line one  \nLine two", "Line one"),
            ("---\nafter a rule\n", "---"),
            ("```rust\nfn main() {}\n```\n", "```rust"),
            ("#\n\nBody", ""),
            ("| a | b |\n|---|---|\n| 1 | 2 |\n", "a b"),
            ("Title\r\nBody\r\n", "Title"),
        ];
        for (body, title) in cases {
            assert_eq!(scan(body).title, title, "{body:?}");
        }
    }

    #[test]
    fn headings_exclude_the_title_line() {
        let body =
            "# Title\n\n## First *part*\n\ntext\n\nSecond\n------\n\n```\n# not a heading\n```\n";
        assert_eq!(scan(body).headings, ["First part", "Second"]);
        assert_eq!(scan("Title\n\n# Heading\n").headings, ["Heading"]);
    }

    #[test]
    fn extracts_tags() {
        let cases: [(&str, &[&str]); 21] = [
            ("#tag", &["tag"]),
            ("Text #one and #two.", &["one", "two"]),
            ("#büro und #Straße", &["büro", "Straße"]),
            ("#arbeit/projekt-a/2026 done", &["arbeit/projekt-a/2026"]),
            ("#a/ trailing slash", &["a"]),
            (
                "(#paren) [#bracket] \"#quote\" „#deutsch“",
                &["paren", "bracket", "quote", "deutsch"],
            ),
            ("#Tag #tag #TAG", &["Tag"]),
            ("#123 and #1a are not tags", &[]),
            ("word#tag and a#b", &[]),
            ("https://example.com/page#section", &[]),
            ("<https://example.com/#top>", &[]),
            ("[text](https://example.com/#frag)", &[]),
            ("[text](#anchor)", &[]),
            ("`#code` and ``#more``", &[]),
            ("```\n#fenced\n```\n\n    #indented\n", &[]),
            ("# Heading\n\n## Second #real\n", &["real"]),
            ("#heading-without-space\n", &["heading-without-space"]),
            ("\\#escaped", &[]),
            ("[[#heading]] and [[Note#part]]", &[]),
            ("- [ ] task #todo\n> quote #zitat\n", &["todo", "zitat"]),
            ("| a | b |\n|---|---|\n| #cell | x |\n", &["cell"]),
        ];
        for (body, tags) in cases {
            assert_eq!(scan(body).tags, tags, "{body:?}");
        }
    }

    #[test]
    fn merges_tags_keeping_the_first_spelling() {
        let frontmatter = ["Work".to_owned(), "x".to_owned()];
        let text = ["work".to_owned(), "Büro".to_owned(), "X".to_owned()];
        assert_eq!(merge_tags(&frontmatter, &text), ["Work", "x", "Büro"]);
        assert!(is_tag("büro/2026"));
        assert!(!is_tag("2026"));
        assert!(!is_tag("a b"));
        assert!(!is_tag(""));
    }

    #[test]
    fn finds_tasks_with_their_markers() {
        let body = "\
List\n\
- [ ] first *item*\n\
- [x] second\n\
  - [ ] nested\n\
    more text\n\
- plain\n\
\n\
> - [X] quoted\n\
\n\
1. [ ] ordered\n\
\n\
```\n\
- [ ] in code\n\
```\n\
\n\
- [ ] loose\n\
\n\
  with a paragraph\n";
        let tasks = scan(body).tasks;
        let seen: Vec<(u32, bool, &str)> = tasks
            .iter()
            .map(|task| (task.number, task.checked, task.text.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (1, false, "first item"),
                (2, true, "second"),
                (3, false, "nested more text"),
                (4, true, "quoted"),
                (5, false, "ordered"),
                (6, false, "loose"),
            ]
        );
        let markers: Vec<&str> = tasks
            .iter()
            .map(|task| &body[task.marker.clone()])
            .collect();
        assert_eq!(markers, ["[ ]", "[x]", "[ ]", "[X]", "[ ]", "[ ]"]);
    }

    #[test]
    fn collects_wiki_link_targets() {
        let body =
            "[[Target]] [[Other|label]] [[Target#heading]] [[ Spaced ]] `[[code]]` [[#local]]";
        assert_eq!(scan(body).links, ["Target", "Other", "Spaced"]);
    }

    #[test]
    fn flags_images_tables_and_attachments() {
        let cases = [
            // (body, image, table, attachment)
            ("plain", false, false, false),
            ("![alt](_attachments/01/photo.PNG)", true, false, false),
            ("![remote](https://example.com/a)", true, false, false),
            ("| a |\n|---|\n| 1 |\n", false, true, false),
            ("[report](_attachments/01/report.pdf)", false, false, true),
            ("[report](files/report.pdf#page=2)", false, false, true),
            ("![scan](scan.pdf)", false, false, true),
            (
                "[note](Other.md) [web](https://example.com/x.pdf)",
                false,
                false,
                false,
            ),
            (
                "[abs](/tmp/x.pdf) [anchor](#top) [mail](mailto:a@b.c)",
                false,
                false,
                false,
            ),
            ("[picture](photo.jpg)", false, false, false),
            ("![[Embedded note]] ![[photo.webp]]", true, false, false),
            ("`![no](x.png)`", false, false, false),
        ];
        for (body, image, table, attachment) in cases {
            let scan = scan(body);
            assert_eq!(
                (scan.has_image, scan.has_table, scan.has_attachment),
                (image, table, attachment),
                "{body:?}"
            );
        }
    }
}

//! The miniature of a note on its card: the first lines of its text, read as blocks
//! (headings, tasks, tags, text), and the image it starts with as a cover. Pure, so
//! that it can run on the background executor and be tested without a window.

use sheaf_core::i18n::tr;
use sheaf_core::snippet::{body_without_title, plain_text};

/// More lines than fit on a card, even without a cover, counted as the widest card
/// wraps them. What is left out costs layout in every frame the card is drawn.
const MAX_LINES: usize = 10;
/// A line longer than this wraps past the bottom of the card anyway.
const MAX_LINE_CHARS: usize = 240;
/// The most characters a line of the widest card holds: 388 px of 11.5 px text, at
/// 5.2 px for a narrow character.
const WIDEST_LINE_CHARS: usize = 75;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// The image the note starts with, as written in the link: relative to the
    /// note's folder, absolute, or a URL.
    pub cover: Option<String>,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Heading(String),
    Task {
        text: String,
        done: bool,
    },
    /// A line of nothing but `#tags`.
    Tags(String),
    Text(String),
}

impl Preview {
    /// Reads the beginning of `body`, the Markdown of a note with its title line.
    pub fn build(body: &str) -> Preview {
        let mut preview = Preview::default();
        let mut fenced = false;
        let mut first = true;
        let mut wrapped = 0;
        for raw in body_without_title(body).lines() {
            if wrapped >= MAX_LINES {
                break;
            }
            let line = raw.trim();
            if line.starts_with("```") || line.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if line.is_empty() {
                continue;
            }
            if fenced {
                wrapped += preview.push(Line::Text(line.to_owned()));
                continue;
            }
            if let Some(source) = image(line) {
                if first {
                    preview.cover = Some(source.to_owned());
                }
                first = false;
                continue;
            }
            first = false;
            if let Some(line) = block(line) {
                wrapped += preview.push(line);
            }
        }
        preview
    }

    /// Adds `line` and returns how many lines of the widest card it takes.
    fn push(&mut self, line: Line) -> usize {
        let shorten = |text: String| match text.char_indices().nth(MAX_LINE_CHARS) {
            Some((end, _)) => text[..end].to_owned(),
            None => text,
        };
        let line = match line {
            Line::Heading(text) => Line::Heading(shorten(text)),
            Line::Task { text, done } => Line::Task {
                text: shorten(text),
                done,
            },
            Line::Tags(text) => Line::Tags(shorten(text)),
            Line::Text(text) => Line::Text(shorten(text)),
        };
        let (Line::Heading(text) | Line::Task { text, .. } | Line::Tags(text) | Line::Text(text)) =
            &line;
        let wrapped = text.chars().count().div_ceil(WIDEST_LINE_CHARS).max(1);
        self.lines.push(line);
        wrapped
    }

    /// One line for a list row: the open tasks, else the first text.
    pub fn summary(&self) -> String {
        let open: Vec<&str> = self
            .lines
            .iter()
            .filter_map(|line| match line {
                Line::Task { text, done: false } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !open.is_empty() {
            return open.join(", ");
        }
        self.lines
            .iter()
            .find_map(|line| match line {
                Line::Text(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tr("No additional text"))
    }
}

/// The source of a line that is a single image, `![alt](source "title")`.
fn image(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("![")?;
    let (_, target) = rest.split_once("](")?;
    let target = target.strip_suffix(')')?;
    let source = target.split_whitespace().next()?;
    let source = source.trim_start_matches('<').trim_end_matches('>');
    (!source.is_empty()).then_some(source)
}

fn block(line: &str) -> Option<Line> {
    // Tables and rules are too wide for a card.
    if line.starts_with('|') || line.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) {
        return None;
    }
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&level) && line[level..].starts_with([' ', '\t']) {
        return Some(Line::Heading(plain_text(&line[level..])));
    }
    let item = ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| line.strip_prefix(bullet))
        .map(str::trim_start);
    if let Some(item) = item {
        for (marker, done) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
            if let Some(text) = item.strip_prefix(marker)
                && (text.is_empty() || text.starts_with([' ', '\t']))
            {
                return Some(Line::Task {
                    text: plain_text(text),
                    done,
                });
            }
        }
    }
    let tags_only = line
        .split_whitespace()
        .all(|word| word.len() > 1 && word.starts_with('#') && !word[1..].starts_with('#'));
    if tags_only {
        return Some(Line::Tags(
            line.split_whitespace().collect::<Vec<_>>().join(" "),
        ));
    }
    let text = plain_text(line);
    (!text.is_empty()).then_some(Line::Text(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &str) -> Line {
        Line::Text(line.to_owned())
    }

    fn task(line: &str, done: bool) -> Line {
        Line::Task {
            text: line.to_owned(),
            done,
        }
    }

    #[test]
    fn a_checklist_becomes_tasks() {
        let preview = Preview::build(
            "Einkauf\n\n- [ ] Milch\n- [ ] Geschenk für Jukek\n- [x] Kaffeebohnen\n* [X] Backpapier\n",
        );
        assert_eq!(preview.cover, None);
        assert_eq!(
            preview.lines,
            [
                task("Milch", false),
                task("Geschenk für Jukek", false),
                task("Kaffeebohnen", true),
                task("Backpapier", true),
            ]
        );
        assert_eq!(preview.summary(), "Milch, Geschenk für Jukek");
    }

    #[test]
    fn headings_text_and_tags_keep_their_kind_and_lose_markup() {
        let preview = Preview::build(
            "# Project plan\n\nKickoff next week. Owner: ==Maya==. #project\n\n## Milestones\n\n\
1. Write the brief\n- Book the **venue**\n\n| a | b |\n|---|---|\n\n---\n\nSee [[Notch-Konzept]] and [docs](https://x.org).\n\n#project #work\n",
        );
        assert_eq!(
            preview.lines,
            [
                text("Kickoff next week. Owner: Maya. #project"),
                Line::Heading("Milestones".to_owned()),
                text("1. Write the brief"),
                text("Book the venue"),
                text("See Notch-Konzept and docs."),
                Line::Tags("#project #work".to_owned()),
            ]
        );
        assert_eq!(
            preview.summary(),
            "Kickoff next week. Owner: Maya. #project"
        );
    }

    #[test]
    fn a_leading_image_is_the_cover_and_a_later_one_is_left_out() {
        let preview =
            Preview::build("Shakshuka\n\n![Pfanne](bilder/pfanne.png \"Titel\")\n\nFür 2.\n");
        assert_eq!(preview.cover.as_deref(), Some("bilder/pfanne.png"));
        assert_eq!(preview.lines, [text("Für 2.")]);

        let later = Preview::build("Notiz\n\nErst Text.\n\n![x](<a b.png>)\n");
        assert_eq!(later.cover, None);
        assert_eq!(later.lines, [text("Erst Text.")]);
    }

    #[test]
    fn code_is_shown_without_fences_and_lines_are_capped() {
        let preview = Preview::build("Code\n\n```sh\ncargo test\n```\n");
        assert_eq!(preview.lines, [text("cargo test")]);

        let long = format!("Lang\n\n{}\n", "Wort ".repeat(200));
        let lines = Preview::build(&long).lines;
        assert!(matches!(&lines[0], Line::Text(line) if line.chars().count() == MAX_LINE_CHARS));

        let many: String = (0..30).map(|n| format!("Zeile {n}\n")).collect();
        assert_eq!(Preview::build(&format!("T\n{many}")).lines.len(), MAX_LINES);
    }

    #[test]
    fn long_lines_count_as_the_lines_they_wrap_into() {
        // Each takes four lines of the widest card: three of them fill it.
        let paragraph = "Wort ".repeat(60);
        let body = format!("T\n\n{paragraph}\n\n{paragraph}\n\n{paragraph}\n\nmehr\n");
        let lines = Preview::build(&body).lines;
        assert_eq!(lines.len(), 3);

        let short = "- [ ] Milch\n".repeat(4);
        let mixed = format!("T\n\n{paragraph}\n\n{paragraph}\n{short}");
        assert_eq!(Preview::build(&mixed).lines.len(), 2 + 2);
    }

    #[test]
    fn an_empty_note_has_nothing_to_show() {
        let preview = Preview::build("Nur ein Titel");
        assert_eq!(preview, Preview::default());
        assert_eq!(preview.summary(), "No additional text");
        assert_eq!(Preview::build("").lines, []);
        // A lone `#` or `[ ]` is text, not a tag or a task.
        assert_eq!(
            Preview::build("T\n\n# \n- [ ]x\n").lines,
            [text("#"), text("[ ]x")]
        );
    }
}

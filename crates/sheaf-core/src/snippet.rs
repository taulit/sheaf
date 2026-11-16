//! Plain-text excerpts of notes: the two lines under a title on a card or in a result.
//!
//! An excerpt is cut from the note's text without the title line and without Markdown
//! markers, so it reads like prose. For a search hit it is centred on the place where
//! most of the query matches, and tells which bytes to highlight.

use std::ops::Range;

use serde::Serialize;

use crate::search::{Mark, Matcher};

/// Length of a snippet in characters, not counting the ellipses: about two card lines.
pub const SNIPPET_CHARS: usize = 160;

/// Only this much of a note is looked at for a snippet, so that a huge note cannot
/// stall a search. A match further down is still found; the snippet then shows the
/// beginning of the note.
const SCAN_LIMIT_BYTES: usize = 128 * 1024;

/// A note is looked at in pieces of about this size, and the search for a snippet
/// stops at the first piece that has all of the query in one window. Most snippets
/// therefore cost one piece, however long the note is.
const CHUNK_BYTES: usize = 4096;

/// Consecutive pieces overlap by this much, so that a window that straddles the end
/// of one piece is found whole in the next.
const CHUNK_OVERLAP_BYTES: usize = 512;

/// An excerpt of a note's text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Snippet {
    /// Plain text, a single line, with `…` where it was cut.
    pub text: String,
    /// Byte ranges of `text` that match the query; empty if only the title matched.
    pub highlights: Vec<Range<usize>>,
}

/// The text below the title line, which is the first line that is not blank.
pub fn body_without_title(body: &str) -> &str {
    let mut rest = body;
    loop {
        let (line, tail) = rest.split_once('\n').unwrap_or((rest, ""));
        if !line.trim().is_empty() || tail.is_empty() {
            return tail;
        }
        rest = tail;
    }
}

/// The beginning of a note's text as plain text: what a card shows under the title.
pub fn preview(body: &str, max_chars: usize) -> String {
    beginning(body_without_title(body), max_chars)
}

/// The first `max_chars` characters of `markdown` as plain text.
fn beginning(markdown: &str, max_chars: usize) -> String {
    // Markup can be much longer than what it shows, so read on until there is enough.
    let mut end = 0;
    loop {
        end = chunk_end(markdown, (end * 2).max(CHUNK_BYTES).max(max_chars * 2));
        let plain = plain_text(&markdown[..end]);
        if end == markdown.len() || plain.chars().nth(max_chars).is_some() {
            return head(&plain, max_chars);
        }
    }
}

/// The snippet of a search hit: the first place in the text where as much of the
/// query as possible occurs together, or the beginning of the text if none of it does.
pub(crate) fn build(body: &str, matcher: &Matcher) -> Snippet {
    let rest = body_without_title(body);
    let rest = &rest[..rest.floor_char_boundary(SCAN_LIMIT_BYTES)];
    let mut best: Option<(u32, Snippet)> = None;
    let mut start = 0;
    while start < rest.len() && !matcher.is_empty() {
        let next = chunk_end(rest, start + CHUNK_BYTES);
        let end = chunk_end(rest, next + CHUNK_OVERLAP_BYTES);
        let plain = plain_text(&rest[start..end]);
        let marks = matcher.marks(&plain);
        if !marks.is_empty() {
            let (terms, snippet) = window(&plain, &marks, start > 0, end < rest.len());
            if best.as_ref().is_none_or(|(most, _)| terms > *most) {
                best = Some((terms, snippet));
            }
            if terms as usize >= matcher.num_terms() {
                break;
            }
        }
        start = next;
    }
    best.map_or_else(
        || Snippet {
            text: beginning(rest, SNIPPET_CHARS),
            highlights: Vec::new(),
        },
        |(_, snippet)| snippet,
    )
}

/// The end of a piece of `text` that should be about `at` bytes long: the next line
/// start, or failing that the next blank, so that neither words nor (usually) Markdown
/// constructs are cut in two.
fn chunk_end(text: &str, at: usize) -> usize {
    if at >= text.len() {
        return text.len();
    }
    let at = text.ceil_char_boundary(at);
    let nearby = &text[at..text.floor_char_boundary(at + CHUNK_OVERLAP_BYTES)];
    let boundary = nearby
        .find('\n')
        .or_else(|| nearby.find(char::is_whitespace));
    boundary.map_or(at, |offset| at + offset + 1)
}

/// The first `max_chars` characters of `text`, cut at a word boundary.
fn head(text: &str, max_chars: usize) -> String {
    let Some((cut, _)) = text.char_indices().nth(max_chars) else {
        return text.to_owned();
    };
    let mut kept = &text[..cut];
    if !text[cut..].starts_with(char::is_whitespace) {
        kept = kept
            .rfind(char::is_whitespace)
            .map_or(kept, |end| &kept[..end]);
    }
    format!("{}…", kept.trim_end())
}

/// Cuts the first [`SNIPPET_CHARS`] of `text` that hold the most different query
/// terms, and tells how many that are. `marks` must be in text order and not empty.
/// `more_before` and `more_after` say whether the note goes on beyond `text`.
fn window(text: &str, marks: &[Mark], more_before: bool, more_after: bool) -> (u32, Snippet) {
    let mut counted = (0, 0);
    let mut char_offset = |byte: usize| {
        counted = (byte, counted.1 + text[counted.0..byte].chars().count());
        counted.1
    };
    let spans: Vec<(usize, usize)> = marks
        .iter()
        .map(|mark| (char_offset(mark.range.start), char_offset(mark.range.end)))
        .collect();
    let total_chars = char_offset(text.len());

    let mut best = (0, 0, 0);
    for first in 0..marks.len() {
        let mut seen = 0u64;
        for last in first..marks.len() {
            if spans[last].1 - spans[first].0 > SNIPPET_CHARS {
                break;
            }
            seen |= 1 << marks[last].term.min(63);
            if seen.count_ones() > best.0 {
                best = (seen.count_ones(), first, last);
            }
        }
    }
    let (terms, first, last) = best;

    // Centre the matches, then give context the note does not have after them to the
    // text before them.
    let slack = SNIPPET_CHARS.saturating_sub(spans[last].1 - spans[first].0);
    let end = (spans[first].0.saturating_sub(slack / 2) + SNIPPET_CHARS).min(total_chars);
    let start = end.saturating_sub(SNIPPET_CHARS);
    let byte_offset = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(byte, _)| byte)
    };
    let (mut from, mut to) = (byte_offset(start), byte_offset(end));

    let (first_match, last_match) = (marks[first].range.start, marks[last].range.end);
    if from > 0 && !text[..from].ends_with(char::is_whitespace) {
        from = text[from..first_match]
            .find(char::is_whitespace)
            .map_or(first_match, |space| from + space);
    }
    from += text[from..].len() - text[from..].trim_start().len();
    if to < text.len() && !text[to..].starts_with(char::is_whitespace) {
        to = text[last_match..to]
            .rfind(char::is_whitespace)
            .map_or(last_match, |space| last_match + space);
    }
    let kept = text[from..to].trim_end();

    let lead = if from > 0 || more_before { "…" } else { "" };
    let trail = if from + kept.len() < text.len() || more_after {
        "…"
    } else {
        ""
    };
    let highlights = marks
        .iter()
        .filter(|mark| mark.range.start >= from && mark.range.end <= from + kept.len())
        .map(|mark| mark.range.start - from + lead.len()..mark.range.end - from + lead.len())
        .collect();
    let snippet = Snippet {
        text: format!("{lead}{kept}{trail}"),
        highlights,
    };
    (terms, snippet)
}

/// Markdown reduced to the text a reader sees, on a single line: no heading, list,
/// quote or emphasis markers, links replaced by their label, table cells separated by
/// spaces. Not a renderer; it only has to read naturally in two lines of a card.
pub fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut fenced = false;
    for line in markdown.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        push_space(&mut out);
        if fenced {
            line.split_whitespace().for_each(|word| {
                out.push_str(word);
                out.push(' ');
            });
        } else if let Some((text, table)) = strip_block_markers(line) {
            strip_inline(text, table, &mut out);
        }
    }
    out.truncate(out.trim_end().len());
    out
}

fn push_space(out: &mut String) {
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
}

/// The content of a line without quote, heading, list and task markers, and whether
/// it is a table row. `None` for lines that are only decoration (rules, table rulers).
fn strip_block_markers(line: &str) -> Option<(&str, bool)> {
    let mut text = line;
    while let Some(rest) = text.strip_prefix('>') {
        text = rest.trim_start();
    }
    let decoration = |c: char| matches!(c, '-' | '*' | '_' | '=' | '|' | ':' | ' ');
    if text.chars().all(decoration) {
        return None;
    }
    if text.starts_with('|') {
        return Some((text, true));
    }
    let level = text.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&level) && text[level..].starts_with([' ', '\t']) {
        return Some((text[level..].trim_start(), false));
    }
    let bullet = ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| text.strip_prefix(bullet));
    if let Some(item) = bullet {
        text = item.trim_start();
        let task = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find_map(|task| text.strip_prefix(task));
        text = task.map_or(text, str::trim_start);
    }
    Some((text, false))
}

/// Appends `text` without inline Markdown markers.
fn strip_inline(text: &str, table: bool, out: &mut String) {
    let mut index = 0;
    loop {
        // Most of a text is neither markup nor blank, and is copied as it is.
        let plain = text.as_bytes()[index..]
            .iter()
            .position(|byte| byte.is_ascii_whitespace() || br"\![<`*~=_|".contains(byte))
            .unwrap_or(text.len() - index);
        out.push_str(&text[index..index + plain]);
        index += plain;
        let Some(c) = text[index..].chars().next() else {
            break;
        };
        let rest = &text[index..];
        let after = &rest[c.len_utf8()..];
        // A marker between two blanks is no marker: `2 * 3`, `a == b`.
        let free_standing = |marker_len: usize| {
            let before = text[..index].chars().next_back();
            let behind = rest[marker_len..].chars().next();
            before.is_none_or(char::is_whitespace) && behind.is_none_or(char::is_whitespace)
        };
        let consumed = match c {
            '\\' if after.starts_with(|next: char| next.is_ascii_punctuation()) => {
                out.push_str(&after[..1]);
                Some(2)
            }
            '!' if after.starts_with('[') => link(after).map(|(label, len)| {
                strip_inline(label, false, out);
                1 + len
            }),
            '[' => link(rest).map(|(label, len)| {
                strip_inline(label, false, out);
                len
            }),
            '<' => angle(rest).map(|(shown, len)| {
                if shown.is_empty() {
                    push_space(out);
                } else {
                    out.push_str(shown);
                }
                len
            }),
            '`' => Some(1),
            '*' if !free_standing(1) => Some(1),
            '~' | '=' if after.starts_with(c) && !free_standing(2) => Some(2),
            '_' => {
                let inside_word = text[..index].ends_with(char::is_alphanumeric)
                    && after.starts_with(char::is_alphanumeric);
                (!inside_word && !free_standing(1)).then_some(1)
            }
            '|' if table => {
                push_space(out);
                Some(1)
            }
            _ if c.is_whitespace() => {
                push_space(out);
                Some(c.len_utf8())
            }
            _ => None,
        };
        index += consumed.unwrap_or_else(|| {
            out.push(c);
            c.len_utf8()
        });
    }
}

/// A link at the start of `text`, which begins with `[`: its label and its length in
/// bytes. Knows `[[target]]`, `[[target|label]]`, `[label](url)` and `[label][ref]`.
fn link(text: &str) -> Option<(&str, usize)> {
    if let Some(inner) = text.strip_prefix("[[") {
        let end = inner.find("]]")?;
        let target = &inner[..end];
        let label = target.rsplit_once('|').map_or(target, |(_, label)| label);
        return Some((label, end + 4));
    }
    let close = text.find(']')?;
    let after = &text[close + 1..];
    let closing = match after.chars().next()? {
        '(' => ')',
        '[' => ']',
        _ => return None,
    };
    let end = after.find(closing)?;
    Some((&text[1..close], close + 1 + end + 1))
}

/// An autolink or HTML tag at the start of `text`, which begins with `<`: what to show
/// for it (nothing for a tag) and its length in bytes.
fn angle(text: &str) -> Option<(&str, usize)> {
    let end = text.find('>')?;
    let inner = &text[1..end];
    if inner.is_empty() || inner.contains(|c: char| c.is_whitespace() || c == '<') {
        return None;
    }
    if inner.contains("://") || inner.contains('@') {
        Some((inner, end + 1))
    } else if inner.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
        Some(("", end + 1))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::parse;

    #[test]
    fn the_title_is_the_first_line_that_is_not_blank() {
        let cases = [
            ("", ""),
            ("Titel", ""),
            ("Titel\n", ""),
            ("Titel\nText", "Text"),
            ("# Titel\n\nText\nmehr", "\nText\nmehr"),
            ("\n  \n# Titel\nText", "Text"),
            ("\n\n", ""),
        ];
        for (body, rest) in cases {
            assert_eq!(body_without_title(body), rest, "{body:?}");
        }
    }

    #[test]
    fn markdown_markers_are_stripped() {
        let cases = [
            ("## Überschrift", "Überschrift"),
            ("#tag bleibt", "#tag bleibt"),
            ("- Punkt\n* Punkt\n+ Punkt", "Punkt Punkt Punkt"),
            ("1. Erstens\n2. Zweitens", "1. Erstens 2. Zweitens"),
            (
                "- [ ] offen\n- [x] erledigt\n* [X] auch",
                "offen erledigt auch",
            ),
            ("> Zitat\n> > tief", "Zitat tief"),
            (
                "**fett** und *kursiv* und ***beides***",
                "fett und kursiv und beides",
            ),
            (
                "__fett__ und _kursiv_, aber snake_case",
                "fett und kursiv, aber snake_case",
            ),
            ("2 * 3 = 6 und a == b", "2 * 3 = 6 und a == b"),
            (
                "`code` und ~~weg~~ und ==wichtig==",
                "code und weg und wichtig",
            ),
            ("```rust\nlet x = 1;\n```\ndanach", "let x = 1; danach"),
            ("[Text](https://example.org/a_b) danach", "Text danach"),
            ("[Text][ref] danach", "Text danach"),
            ("![Bild](bild.png) danach", "Bild danach"),
            ("![](bild.png) danach", "danach"),
            (
                "[[Ziel]] und [[Ziel|Name]] und [[Ziel#Teil|Name]]",
                "Ziel und Name und Name",
            ),
            ("[ ] kein Link [x]", "[ ] kein Link [x]"),
            (
                "<https://example.org> und <a@b.de>",
                "https://example.org und a@b.de",
            ),
            ("Zeile<br>Umbruch <b>fett</b>", "Zeile Umbruch fett"),
            ("a < b > c", "a < b > c"),
            ("\\*kein\\* \\# Markup", "*kein* # Markup"),
            ("| A | B |\n|---|:-:|\n| 1 | 2 |", "A B 1 2"),
            ("Text\n\n---\n***\n===\nmehr", "Text mehr"),
            (
                "  viel   Leerraum\t hier  \n\n\n  und   da ",
                "viel Leerraum hier und da",
            ),
            ("**[Link](x)** _[[Ziel]]_", "Link Ziel"),
            ("", ""),
        ];
        for (markdown, plain) in cases {
            assert_eq!(plain_text(markdown), plain, "{markdown:?}");
        }
    }

    #[test]
    fn previews_are_cut_at_word_boundaries() {
        let body = "Titel\n\nEins zwei drei vier fünf";
        assert_eq!(preview(body, 100), "Eins zwei drei vier fünf");
        assert_eq!(preview(body, 24), "Eins zwei drei vier fünf");
        assert_eq!(preview(body, 23), "Eins zwei drei vier…");
        assert_eq!(preview(body, 14), "Eins zwei drei…");
        assert_eq!(preview(body, 12), "Eins zwei…");
        assert_eq!(preview(body, 3), "Ein…");
        assert_eq!(preview("Titel", 10), "");
    }

    #[test]
    fn a_short_text_is_shown_whole() {
        let matcher = Matcher::new(&parse("zwei"));
        let snippet = build("Titel\n\nEins **zwei** drei", &matcher);
        assert_eq!(snippet.text, "Eins zwei drei");
        assert_eq!(snippet.highlights.len(), 1);
        assert_eq!(snippet.highlights[0], 5..9);
    }

    #[test]
    fn a_match_in_the_title_line_is_not_a_match_in_the_snippet() {
        let matcher = Matcher::new(&parse("titel"));
        let snippet = build("Titel\n\nEins zwei drei", &matcher);
        assert_eq!(snippet.text, "Eins zwei drei");
        assert!(snippet.highlights.is_empty());
    }

    #[test]
    fn the_window_prefers_all_terms_once_over_one_term_often() {
        let filler = "wort ".repeat(60);
        let body = format!("Titel\n\nalpha alpha alpha alpha {filler}alpha beta {filler}");
        let snippet = build(&body, &Matcher::new(&parse("alpha beta")));
        let marked: Vec<&str> = snippet
            .highlights
            .iter()
            .map(|range| &snippet.text[range.clone()])
            .collect();
        assert_eq!(marked, ["alpha", "beta"]);
        assert!(snippet.text.starts_with("…wort") && snippet.text.ends_with("wort…"));
    }

    #[test]
    fn a_window_at_the_end_of_the_text_uses_the_space_before_it() {
        let body = format!("Titel\n\n{}ganz am Schluss", "wort ".repeat(60));
        let snippet = build(&body, &Matcher::new(&parse("schluss")));
        assert!(snippet.text.ends_with("wort ganz am Schluss"));
        assert_eq!(snippet.text.chars().count(), SNIPPET_CHARS + 1);
        let range = snippet.highlights[0].clone();
        assert_eq!(&snippet.text[range], "Schluss");
    }

    #[test]
    fn multibyte_text_is_measured_in_characters() {
        let body = format!(
            "Titel\n\n{}später größer {}",
            "äöü ".repeat(80),
            "ßßß ".repeat(80)
        );
        let snippet = build(&body, &Matcher::new(&parse("grosser")));
        let range = snippet.highlights[0].clone();
        assert_eq!(&snippet.text[range], "größer");
        let inner = snippet.text.trim_matches('…');
        assert!(inner.chars().count() <= SNIPPET_CHARS);
        assert!(inner.chars().count() > SNIPPET_CHARS - 8);
    }

    fn marked(snippet: &Snippet) -> Vec<&str> {
        snippet
            .highlights
            .iter()
            .map(|range| &snippet.text[range.clone()])
            .collect()
    }

    #[test]
    fn a_match_deep_in_a_long_note_is_found() {
        let line = "Lorem ipsum dolor sit amet. ".repeat(40);
        let body = format!(
            "Titel\n\n{}Hier steht die Nadel im Heu.\n{line}\n",
            format!("{line}\n").repeat(20)
        );
        assert!(body.len() > 5 * CHUNK_BYTES);
        let snippet = build(&body, &Matcher::new(&parse("nadel")));
        assert_eq!(marked(&snippet), ["Nadel"]);
        assert!(
            snippet
                .text
                .contains("amet. Hier steht die Nadel im Heu. Lorem")
        );
        assert!(snippet.text.starts_with('…') && snippet.text.ends_with('…'));
        assert!(snippet.text.chars().count() <= SNIPPET_CHARS + 2);
    }

    #[test]
    fn the_whole_query_further_down_beats_a_part_of_it_at_the_top() {
        let line = "wort ".repeat(200);
        let body = format!(
            "Titel\n\nalpha am Anfang\n{}alpha und beta zusammen\n{line}\n",
            format!("{line}\n").repeat(12)
        );
        let snippet = build(&body, &Matcher::new(&parse("alpha beta")));
        assert_eq!(marked(&snippet), ["alpha", "beta"]);

        // Without such a place the first match is shown.
        let snippet = build(&body, &Matcher::new(&parse("alpha gamma")));
        assert_eq!(marked(&snippet), ["alpha"]);
        assert!(snippet.text.starts_with("alpha am Anfang wort"));
    }

    #[test]
    fn matches_on_both_sides_of_a_piece_boundary_are_found_together() {
        let line = format!("{}\n", "wort ".repeat(200));
        let before = format!("{}{}alpha\n", line.repeat(4), "wort ".repeat(20));
        assert!((CHUNK_BYTES..CHUNK_BYTES + CHUNK_OVERLAP_BYTES).contains(&before.len()));
        assert_eq!(chunk_end(&before, CHUNK_BYTES), before.len());
        let body = format!("Titel\n\n{before}beta {line}{line}");
        let snippet = build(&body, &Matcher::new(&parse("alpha beta")));
        assert_eq!(marked(&snippet), ["alpha", "beta"]);
    }

    #[test]
    fn pieces_end_at_line_starts_or_blanks() {
        let lines = "abc def\n".repeat(1000);
        assert_eq!(chunk_end(&lines, 0), 8);
        assert_eq!(chunk_end(&lines, 8), 16);
        assert_eq!(chunk_end(&lines, 9), 16);
        assert_eq!(chunk_end(&lines, lines.len()), lines.len());
        assert_eq!(chunk_end(&lines, lines.len() + 100), lines.len());
        let one_line = "äöü ".repeat(1000);
        assert_eq!(chunk_end(&one_line, 1), 7);
        let one_word = "ä".repeat(1000);
        assert_eq!(chunk_end(&one_word, 1), 2);
        assert_eq!(chunk_end("", 0), 0);
    }

    #[test]
    fn a_long_preview_reads_as_much_as_it_needs() {
        let body = format!(
            "Titel\n\n{}",
            "[wort](https://example.org/a/very/long/address) ".repeat(2000)
        );
        let preview = preview(&body, 3000);
        assert_eq!(preview.chars().count(), 3000);
        assert!(preview.starts_with("wort wort wort") && preview.ends_with("wort…"));
    }

    #[test]
    fn only_the_beginning_of_a_huge_note_is_scanned() {
        let body = format!(
            "Titel\n\nAnfang {}Nadel",
            "heu ".repeat(SCAN_LIMIT_BYTES / 4 + 10)
        );
        let snippet = build(&body, &Matcher::new(&parse("nadel")));
        assert!(snippet.text.starts_with("Anfang heu heu"));
        assert!(snippet.highlights.is_empty());
    }
}

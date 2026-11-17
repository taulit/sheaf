//! Lossless handling of the YAML frontmatter block.
//!
//! Sheaf never re-serialises frontmatter. It reads the handful of keys it knows and
//! edits single top-level keys in place, so unknown keys, comments, quoting, line
//! endings and a byte order mark all survive. A block that does not look like the
//! simple `key: value` subset is treated as part of the body and left alone.

use std::ops::Range;

use jiff::Timestamp;

use crate::model::NoteId;

const BOM: char = '\u{feff}';
const FENCE: &str = "---";

/// A note file cut into its parts. Concatenated they are the file again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts<'a> {
    /// The byte order mark, or an empty string.
    pub bom: &'a str,
    /// The frontmatter block with both `---` lines, if the file has a valid one.
    pub block: Option<&'a str>,
    pub body: &'a str,
}

/// The frontmatter keys Sheaf understands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub id: Option<NoteId>,
    pub created: Option<Timestamp>,
    pub pinned: bool,
    /// Tags without `#`, in file order.
    pub tags: Vec<String>,
    /// Set while the note is in the trash.
    pub deleted: Option<Timestamp>,
    /// The folder a trashed note came from.
    pub from: Option<String>,
}

/// Cuts `text` into byte order mark, frontmatter block and body.
pub fn split(text: &str) -> Parts<'_> {
    let rest = text.strip_prefix(BOM).unwrap_or(text);
    let bom = &text[..text.len() - rest.len()];
    let none = Parts {
        bom,
        block: None,
        body: rest,
    };

    let mut lines = rest.split_inclusive('\n');
    if lines.next().map(|line| strip_eol(line).trim_end()) != Some(FENCE) {
        return none;
    }
    let mut end = rest.find('\n').map_or(rest.len(), |at| at + 1);
    let mut seen_key = false;
    for line in lines {
        end += line.len();
        let content = strip_eol(line);
        if content.trim_end() == FENCE {
            return Parts {
                bom,
                block: Some(&rest[..end]),
                body: &rest[end..],
            };
        }
        match classify(content) {
            Line::Blank | Line::Comment => {}
            Line::Key(..) => seen_key = true,
            Line::Continuation if seen_key => {}
            Line::Continuation | Line::Other => return none,
        }
    }
    none
}

impl Parts<'_> {
    /// The known keys of the block; all defaults when there is none.
    pub fn frontmatter(&self) -> Frontmatter {
        let mut frontmatter = Frontmatter::default();
        // Reversed so that the first of two equal keys wins, the one `set` edits.
        for entry in self.block.map(entries).unwrap_or_default().iter().rev() {
            let value = scalar(entry.value);
            match entry.key {
                "id" => {
                    frontmatter.id = value
                        .parse()
                        .ok()
                        .filter(|id: &NoteId| !id.is_provisional());
                }
                "created" => frontmatter.created = value.parse().ok(),
                "deleted" => frontmatter.deleted = value.parse().ok(),
                "pinned" => frontmatter.pinned = value.eq_ignore_ascii_case("true"),
                "from" => frontmatter.from = Some(value),
                "tags" => frontmatter.tags = tags(entry),
                _ => {}
            }
        }
        frontmatter
    }
}

/// Sets a top-level key to `value` (raw YAML) and returns the new file text.
///
/// An existing key has its line replaced, a new key is added as the last line of the
/// block, and a file without frontmatter gets a block.
pub fn set(text: &str, key: &str, value: &str) -> String {
    let parts = split(text);
    let Some(block) = parts.block else {
        let eol = line_ending(text);
        return format!(
            "{}{FENCE}{eol}{key}: {value}{eol}{FENCE}{eol}{}",
            parts.bom, parts.body
        );
    };

    let existing = entries(block).into_iter().find(|entry| entry.key == key);
    let (range, eol) = match existing {
        Some(entry) => {
            let eol = line_ending(&block[entry.range.clone()]);
            (entry.range, eol)
        }
        None => {
            let above = block.strip_suffix('\n').unwrap_or(block);
            let closing = above.rfind('\n').map_or(0, |at| at + 1);
            (closing..closing, line_ending(block))
        }
    };
    let offset = parts.bom.len();
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    out.push_str(&text[..offset + range.start]);
    out.push_str(&format!("{key}: {value}{eol}"));
    out.push_str(&text[offset + range.end..]);
    out
}

/// Removes a top-level key and returns the new file text. The block goes away with its
/// last key.
pub fn remove(text: &str, key: &str) -> String {
    let parts = split(text);
    let Some(block) = parts.block else {
        return text.to_owned();
    };
    let Some(entry) = entries(block).into_iter().find(|entry| entry.key == key) else {
        return text.to_owned();
    };

    let kept = format!(
        "{}{}",
        &block[..entry.range.start],
        &block[entry.range.end..]
    );
    let lines: Vec<&str> = kept.split_inclusive('\n').collect();
    let inner = &lines[1..lines.len() - 1];
    if inner.iter().all(|line| line.trim().is_empty()) {
        format!("{}{}", parts.bom, parts.body)
    } else {
        format!("{}{kept}{}", parts.bom, parts.body)
    }
}

/// `value` as a YAML scalar, double-quoted when a plain scalar could be misread.
pub fn quoted(value: &str) -> String {
    let plain = value
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '/' | '.'))
        && value.starts_with(char::is_alphabetic)
        && !value.ends_with(' ')
        && !["true", "false", "null", "yes", "no", "on", "off"]
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word));
    if plain {
        return value.to_owned();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for c in value.chars() {
        match c {
            '\\' | '"' => quoted.extend(['\\', c]),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// The line ending of the first line: `"\r\n"` or `"\n"`.
pub fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(at) if text[..at].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Puts a body behind the head of a file (byte order mark and frontmatter block).
pub fn join(head: &str, body: &str) -> String {
    let unterminated = !head.is_empty() && !head.ends_with('\n') && !body.is_empty();
    let eol = if unterminated { line_ending(head) } else { "" };
    format!("{head}{eol}{body}")
}

enum Line<'a> {
    Blank,
    Comment,
    /// A top-level key and the text after its colon.
    Key(&'a str, &'a str),
    /// An indented line or a list item; belongs to the key above it.
    Continuation,
    Other,
}

fn classify(content: &str) -> Line<'_> {
    if content.trim().is_empty() {
        Line::Blank
    } else if content.starts_with([' ', '\t']) || content == "-" || content.starts_with("- ") {
        Line::Continuation
    } else if content.starts_with('#') {
        Line::Comment
    } else {
        content
            .match_indices(':')
            .find(|(at, _)| {
                let rest = &content[at + 1..];
                rest.is_empty() || rest.starts_with([' ', '\t'])
            })
            .map(|(at, _)| (content[..at].trim_end(), &content[at + 1..]))
            .filter(|(key, _)| !key.is_empty())
            .map_or(Line::Other, |(key, value)| Line::Key(key, value))
    }
}

/// A top-level key with the lines that belong to it.
struct Entry<'a> {
    key: &'a str,
    value: &'a str,
    continuation: Vec<&'a str>,
    /// Bytes of the entry within the block, line endings included.
    range: Range<usize>,
}

fn entries(block: &str) -> Vec<Entry<'_>> {
    let lines: Vec<&str> = block.split_inclusive('\n').collect();
    let mut entries: Vec<Entry<'_>> = Vec::new();
    let mut open = false;
    let mut offset = lines.first().map_or(0, |fence| fence.len());
    for line in &lines[1..lines.len() - 1] {
        let start = offset;
        offset += line.len();
        match classify(strip_eol(line)) {
            Line::Key(key, value) => {
                entries.push(Entry {
                    key,
                    value,
                    continuation: Vec::new(),
                    range: start..offset,
                });
                open = true;
            }
            Line::Continuation if open => {
                if let Some(entry) = entries.last_mut() {
                    entry.continuation.push(strip_eol(line));
                    entry.range.end = offset;
                }
            }
            _ => open = false,
        }
    }
    entries
}

fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// The value of a scalar: quotes resolved, a trailing comment dropped.
fn scalar(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => value.extend(chars.next().map(|escaped| match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                })),
                _ => value.push(c),
            }
        }
        value
    } else if let Some(rest) = raw.strip_prefix('\'') {
        let end = rest.find('\'').unwrap_or(rest.len());
        rest[..end].to_owned()
    } else if raw.starts_with('#') {
        String::new()
    } else {
        let end = raw.find(" #").unwrap_or(raw.len());
        raw[..end].trim_end().to_owned()
    }
}

fn tags(entry: &Entry<'_>) -> Vec<String> {
    let value = entry.value.trim();
    let items = if let Some(list) = value.strip_prefix('[') {
        flow_items(list)
    } else {
        let single = scalar(value);
        if single.is_empty() {
            entry
                .continuation
                .iter()
                .filter_map(|line| line.trim_start().strip_prefix('-'))
                .map(scalar)
                .collect()
        } else {
            vec![single]
        }
    };
    let mut tags: Vec<String> = Vec::new();
    for item in items {
        let tag = item.trim_start_matches('#').trim();
        if !tag.is_empty() && !tags.iter().any(|known| known.eq_ignore_ascii_case(tag)) {
            tags.push(tag.to_owned());
        }
    }
    tags
}

/// The items of an inline list, given the text after its `[`.
fn flow_items(list: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (at, c) in list.char_indices() {
        match (quote, c) {
            (Some(open), _) if c == open => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, ',' | ']') => {
                items.push(scalar(&list[start..at]));
                start = at + 1;
                if c == ']' {
                    return items;
                }
            }
            _ => {}
        }
    }
    items.push(scalar(&list[start..]));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JD3K8Z6Q4X7V9B2M5N8P0R3T";

    #[test]
    fn splits_into_parts_that_concatenate_to_the_file() {
        let cases = [
            "",
            "Just a note\n",
            "---\nid: x\n---\nBody\n",
            "---\r\nid: x\r\n---\r\nBody\r\n",
            "\u{feff}---\nid: x\n---\nBody",
            "---\nid: x\n---",
            "---\n# only a comment\n\n---\nBody",
            "---\nnot closed\n",
            "---\nA rule, then text\n---\n",
        ];
        for text in cases {
            let parts = split(text);
            let joined = format!("{}{}{}", parts.bom, parts.block.unwrap_or(""), parts.body);
            assert_eq!(joined, text, "{text:?}");
        }
        assert_eq!(split("---\nid: x\n---\nBody\n").body, "Body\n");
        assert_eq!(split("---\nid: x\n---").block, Some("---\nid: x\n---"));
        assert_eq!(split("\u{feff}---\na: 1\n---\nB").bom, "\u{feff}");
    }

    #[test]
    fn broken_frontmatter_is_body() {
        let cases = [
            "---\nnot closed\nid: x\n",
            "---\nA rule, then text\n---\n",
            "---\n  indented: before any key\n---\n",
            "---\n- item\n---\n",
            "---\nhttp://example.com\n---\n",
            " ---\nid: x\n---\n",
            "----\nid: x\n---\n",
        ];
        for text in cases {
            let parts = split(text);
            assert_eq!(parts.block, None, "{text:?}");
            assert_eq!(parts.body, text);
        }
    }

    #[test]
    fn reads_known_keys() {
        let text = format!(
            "---\nid: {ID}\ncreated: 2026-11-16T10:12:00+01:00\npinned: true  # keep\n\
             tags: [büro, \"a b\", '#x']\ndeleted: 2026-12-01T08:00:00Z\nfrom: \"Arbeit/Q4\"\n\
             custom: 1\n---\nBody\n"
        );
        let frontmatter = split(&text).frontmatter();
        assert_eq!(frontmatter.id.unwrap().as_str(), ID);
        assert_eq!(
            frontmatter.created.unwrap(),
            "2026-11-16T09:12:00Z".parse::<Timestamp>().unwrap()
        );
        assert!(frontmatter.pinned);
        assert_eq!(frontmatter.tags, ["büro", "a b", "x"]);
        assert_eq!(
            frontmatter.deleted.unwrap(),
            "2026-12-01T08:00:00Z".parse::<Timestamp>().unwrap()
        );
        assert_eq!(frontmatter.from.as_deref(), Some("Arbeit/Q4"));
    }

    #[test]
    fn reads_every_tag_notation() {
        let cases: [(&str, &[&str]); 8] = [
            ("tags: [a, b]", &["a", "b"]),
            ("tags: []", &[]),
            ("tags:\n  - a\n  - \"b\"\n", &["a", "b"]),
            ("tags:\n- a\n- b # second\n", &["a", "b"]),
            ("tags: single", &["single"]),
            ("tags: \"#quoted\"", &["quoted"]),
            ("tags: [a, A, b]", &["a", "b"]),
            ("tags:", &[]),
        ];
        for (yaml, expected) in cases {
            let text = format!("---\n{}\n---\n", yaml.trim_end());
            assert_eq!(split(&text).frontmatter().tags, expected, "{yaml:?}");
        }
    }

    #[test]
    fn unusable_values_read_as_absent() {
        let text = "---\nid: not-an-id\ncreated: yesterday\npinned: maybe\n---\n";
        assert_eq!(split(text).frontmatter(), Frontmatter::default());
        let provisional = "---\nid: p9e2sp7zkne9vvg1q9ev2b7zxq\n---\n";
        assert_eq!(split(provisional).frontmatter().id, None);
    }

    #[test]
    fn set_edits_only_its_line() {
        let cases = [
            // (before, key, value, after)
            ("Body\n", "id", "X", "---\nid: X\n---\nBody\n"),
            ("", "id", "X", "---\nid: X\n---\n"),
            (
                "Body\r\nMore\r\n",
                "id",
                "X",
                "---\r\nid: X\r\n---\r\nBody\r\nMore\r\n",
            ),
            (
                "\u{feff}Body\n",
                "id",
                "X",
                "\u{feff}---\nid: X\n---\nBody\n",
            ),
            ("---\nid: A\n---\nBody", "id", "B", "---\nid: B\n---\nBody"),
            (
                "---\n# note\nfoo: 'bar'   # why\nid: A\n---\nBody",
                "pinned",
                "true",
                "---\n# note\nfoo: 'bar'   # why\nid: A\npinned: true\n---\nBody",
            ),
            (
                "---\r\nfoo: 1\r\n---\r\nBody",
                "id",
                "X",
                "---\r\nfoo: 1\r\nid: X\r\n---\r\nBody",
            ),
            (
                "---\ntags:\n  - a\n  - b\nfoo: 1\n---\n",
                "tags",
                "[a, b, c]",
                "---\ntags: [a, b, c]\nfoo: 1\n---\n",
            ),
            ("---\nfoo: 1\n---", "id", "X", "---\nfoo: 1\nid: X\n---"),
            (
                "---\nbroken\n---\nBody\n",
                "id",
                "X",
                "---\nid: X\n---\n---\nbroken\n---\nBody\n",
            ),
        ];
        for (before, key, value, after) in cases {
            assert_eq!(set(before, key, value), after, "{before:?}");
        }
    }

    #[test]
    fn remove_edits_only_its_line() {
        let cases = [
            ("Body\n", "pinned", "Body\n"),
            (
                "---\nid: A\n---\nBody\n",
                "pinned",
                "---\nid: A\n---\nBody\n",
            ),
            (
                "---\nid: A\npinned: true\n---\nBody\n",
                "pinned",
                "---\nid: A\n---\nBody\n",
            ),
            ("---\npinned: true\n---\nBody\n", "pinned", "Body\n"),
            (
                "\u{feff}---\r\npinned: true\r\n---\r\nBody\r\n",
                "pinned",
                "\u{feff}Body\r\n",
            ),
            (
                "---\n# keep me\npinned: true\n---\nBody\n",
                "pinned",
                "---\n# keep me\n---\nBody\n",
            ),
            (
                "---\ntags:\n  - a\n  - b\n\nid: A\n---\n",
                "tags",
                "---\n\nid: A\n---\n",
            ),
            ("---\nbroken\n---\n", "broken", "---\nbroken\n---\n"),
        ];
        for (before, key, after) in cases {
            assert_eq!(remove(before, key), after, "{before:?}");
        }
    }

    #[test]
    fn set_then_remove_restores_the_file() {
        let cases = [
            "Body\n",
            "\u{feff}Body\r\nMore\r\n",
            "---\nid: A\nunknown: [1, 2]\n# comment\n---\nBody\n",
            "---\r\nid: A\r\ntags:\r\n  - a\r\n---\r\nBody\r\n",
        ];
        for text in cases {
            assert_eq!(
                remove(&set(text, "pinned", "true"), "pinned"),
                text,
                "{text:?}"
            );
        }
    }

    #[test]
    fn set_value_is_read_back() {
        for folder in [
            "Q4: \"Plan\" #1",
            "back\\slash",
            "line\nbreak\tand tab",
            " padded ",
        ] {
            let text = set("Body\n", "from", &quoted(folder));
            assert_eq!(text.lines().count(), 4, "{text:?}");
            assert_eq!(split(&text).frontmatter().from.as_deref(), Some(folder));
        }
        assert_eq!(quoted("Arbeit/Projekt A"), "Arbeit/Projekt A");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("true"), "\"true\"");
        assert_eq!(quoted("2026"), "\"2026\"");
    }

    #[test]
    fn join_keeps_the_head_on_its_own_line() {
        assert_eq!(join("---\nid: A\n---\n", "Body"), "---\nid: A\n---\nBody");
        assert_eq!(join("---\nid: A\n---", "Body"), "---\nid: A\n---\nBody");
        assert_eq!(
            join("---\r\nid: A\r\n---", "Body"),
            "---\r\nid: A\r\n---\r\nBody"
        );
        assert_eq!(join("---\nid: A\n---", ""), "---\nid: A\n---");
        assert_eq!(join("", "Body"), "Body");
    }
}

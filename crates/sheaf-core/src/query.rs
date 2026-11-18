//! The search language: words, `"quoted phrases"` and filters such as `#tag`,
//! `folder:work` or `has:checklist`.
//!
//! Parsing never fails. Whatever is not a filter is a word to search for. German and
//! English keywords are both accepted, whatever the interface language is.

use std::fmt;
use std::ops::Range;

use serde::Serialize;

use crate::analysis::fold;

/// A search as typed, cut into its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ParsedQuery {
    /// The parts in the order they were typed.
    pub items: Vec<QueryItem>,
}

/// One part of a query and where it stands in the input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QueryItem {
    pub kind: ItemKind,
    /// Byte range in the parsed input, including quotes, `#` and the filter keyword.
    pub span: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ItemKind {
    /// A word that must match exactly, as a prefix or with a typo.
    Word(String),
    /// Quoted text that must occur as written.
    Phrase(String),
    Filter(Filter),
    /// A filter that is still being typed, such as `#`, `folder:` or `has:ch`. It is
    /// kept as typed and does not take part in the search, so that results do not
    /// flicker while the user completes it.
    Pending(String),
}

/// A restriction on the notes a search or listing returns.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Filter {
    /// `#tag`: the note has this tag or one nested below it (`#a` matches `a/b`).
    Tag(String),
    /// `folder:name`: the value equals a run of segments of the note's folder path,
    /// ignoring case and accents. A leading `/` anchors the run at the notes folder.
    Folder(String),
    /// `has:checklist|image|table|attachment`.
    Has(Feature),
    /// `open:`: the note has unchecked tasks.
    Open,
    /// `modified:today|week|month`.
    Modified(Period),
    /// `pinned:`.
    Pinned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Feature {
    Checklist,
    Image,
    Table,
    Attachment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Period {
    /// Since midnight in the user's time zone.
    Today,
    /// The last 7 days.
    Week,
    /// The last 30 days.
    Month,
}

impl ParsedQuery {
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.items.iter().filter_map(|item| match &item.kind {
            ItemKind::Word(word) => Some(word.as_str()),
            _ => None,
        })
    }

    pub fn phrases(&self) -> impl Iterator<Item = &str> {
        self.items.iter().filter_map(|item| match &item.kind {
            ItemKind::Phrase(phrase) => Some(phrase.as_str()),
            _ => None,
        })
    }

    pub fn filters(&self) -> impl Iterator<Item = &Filter> {
        self.items.iter().filter_map(|item| match &item.kind {
            ItemKind::Filter(filter) => Some(filter),
            _ => None,
        })
    }
}

/// The canonical English form. Parsing it gives the same parts again, which is how
/// smart folders store their search.
impl fmt::Display for ParsedQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                f.write_str(" ")?;
            }
            item.kind.fmt(f)?;
        }
        Ok(())
    }
}

impl fmt::Display for ItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemKind::Word(word) | ItemKind::Pending(word) => f.write_str(word),
            ItemKind::Phrase(phrase) => write!(f, "\"{phrase}\""),
            ItemKind::Filter(filter) => filter.fmt(f),
        }
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Filter::Tag(tag) => write!(f, "#{tag}"),
            Filter::Folder(folder) if folder.contains(char::is_whitespace) => {
                write!(f, "folder:\"{folder}\"")
            }
            Filter::Folder(folder) => write!(f, "folder:{folder}"),
            Filter::Has(feature) => write!(f, "has:{}", feature.keyword()),
            Filter::Open => f.write_str("open:"),
            Filter::Modified(period) => write!(f, "modified:{}", period.keyword()),
            Filter::Pinned => f.write_str("pinned:"),
        }
    }
}

impl Feature {
    const NAMES: [(&'static str, Feature); 8] = [
        ("checklist", Feature::Checklist),
        ("checkliste", Feature::Checklist),
        ("image", Feature::Image),
        ("bild", Feature::Image),
        ("table", Feature::Table),
        ("tabelle", Feature::Table),
        ("attachment", Feature::Attachment),
        ("anhang", Feature::Attachment),
    ];

    /// The English keyword used after `has:`.
    pub fn keyword(self) -> &'static str {
        match self {
            Feature::Checklist => "checklist",
            Feature::Image => "image",
            Feature::Table => "table",
            Feature::Attachment => "attachment",
        }
    }
}

impl Period {
    const NAMES: [(&'static str, Period); 6] = [
        ("today", Period::Today),
        ("heute", Period::Today),
        ("week", Period::Week),
        ("woche", Period::Week),
        ("month", Period::Month),
        ("monat", Period::Month),
    ];

    /// The English keyword used after `modified:`.
    pub fn keyword(self) -> &'static str {
        match self {
            Period::Today => "today",
            Period::Week => "week",
            Period::Month => "month",
        }
    }
}

#[derive(Clone, Copy)]
enum Key {
    Folder,
    Has,
    Open,
    Modified,
    Pinned,
}

impl Key {
    fn from_name(name: &str) -> Option<Key> {
        // Folding makes the keyword case-insensitive and accepts "geandert".
        match fold(name).primary.as_str() {
            "folder" | "ordner" => Some(Key::Folder),
            "has" | "hat" => Some(Key::Has),
            "open" | "offen" => Some(Key::Open),
            "modified" | "geandert" | "geaendert" => Some(Key::Modified),
            "pinned" | "angeheftet" => Some(Key::Pinned),
            _ => None,
        }
    }
}

/// Cuts `input` into words, phrases and filters.
pub fn parse(input: &str) -> ParsedQuery {
    let mut items = Vec::new();
    let mut position = 0;
    while let Some(offset) = input[position..].find(|c: char| !c.is_whitespace()) {
        let start = position + offset;
        let (kind, end) = if input[start..].starts_with('"') {
            let (text, end) = quoted(input, start);
            (
                (!text.is_empty()).then(|| ItemKind::Phrase(text.to_owned())),
                end,
            )
        } else {
            let (kind, end) = token(input, start);
            (Some(kind), end)
        };
        if let Some(kind) = kind {
            items.push(QueryItem {
                kind,
                span: start..end,
            });
        }
        position = end;
    }
    ParsedQuery { items }
}

/// The trimmed text between the quote at `open` and the next quote, and the position
/// after it. A missing closing quote, as while typing, ends the text at the input's end.
fn quoted(input: &str, open: usize) -> (&str, usize) {
    let start = open + 1;
    match input[start..].find('"') {
        Some(length) => (input[start..start + length].trim(), start + length + 1),
        None => (input[start..].trim(), input.len()),
    }
}

/// Classifies the token starting at `start` and returns the position after it.
fn token(input: &str, start: usize) -> (ItemKind, usize) {
    let end = input[start..]
        .find(char::is_whitespace)
        .map_or(input.len(), |length| start + length);
    let raw = &input[start..end];
    let word = || ItemKind::Word(raw.to_owned());
    let pending = || ItemKind::Pending(raw.to_owned());

    if let Some(tag) = raw.strip_prefix('#') {
        let tag = tag.trim_matches('/');
        let kind = if tag.chars().any(char::is_alphanumeric) {
            ItemKind::Filter(Filter::Tag(tag.to_owned()))
        } else if raw == "#" {
            pending()
        } else {
            word()
        };
        return (kind, end);
    }

    let Some((name, value)) = raw.split_once(':') else {
        return (word(), end);
    };
    let Some(key) = Key::from_name(name) else {
        return (word(), end);
    };
    let kind = match key {
        Key::Folder => {
            let (value, end) = if value.starts_with('"') {
                quoted(input, start + name.len() + 1)
            } else {
                (value, end)
            };
            let value = value.trim_end_matches('/');
            let kind = if value.is_empty() {
                ItemKind::Pending(input[start..end].to_owned())
            } else {
                ItemKind::Filter(Filter::Folder(value.to_owned()))
            };
            return (kind, end);
        }
        Key::Has => named(&Feature::NAMES, value).map(|found| found.map(Filter::Has)),
        Key::Modified => named(&Period::NAMES, value).map(|found| found.map(Filter::Modified)),
        Key::Open => value.is_empty().then_some(Some(Filter::Open)),
        Key::Pinned => value.is_empty().then_some(Some(Filter::Pinned)),
    };
    let kind = match kind {
        Some(Some(filter)) => ItemKind::Filter(filter),
        Some(None) => pending(),
        None => word(),
    };
    (kind, end)
}

/// The folded segments of a folder path or of a `folder:` value.
fn folder_segments(path: &str) -> Vec<String> {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| fold(segment).primary)
        .collect()
}

/// Every run of consecutive segments of `folder`, so that `folder:b` and `folder:b/c`
/// find a note in `a/b/c` with one term lookup. Runs that start at the root are also
/// listed with a leading `/`, for anchored values.
pub(crate) fn folder_runs(folder: &str) -> Vec<String> {
    let segments = folder_segments(folder);
    let mut runs = Vec::new();
    for start in 0..segments.len() {
        for end in start + 1..=segments.len() {
            let run = segments[start..end].join("/");
            if start == 0 {
                runs.push(format!("/{run}"));
            }
            runs.push(run);
        }
    }
    runs
}

/// The term a `folder:` value looks up among the [`folder_runs`] of a note's folder.
pub(crate) fn folder_key(value: &str) -> String {
    let run = folder_segments(value).join("/");
    if value.starts_with('/') {
        format!("/{run}")
    } else {
        run
    }
}

/// Whether the `folder:` value `value` finds the notes in `folder` (and so in the
/// folders inside it): the rule the search index applies.
pub fn folder_matches(value: &str, folder: &str) -> bool {
    folder_runs(folder).contains(&folder_key(value))
}

/// `query` with every `folder:` filter that found the folder `from` (or one inside it),
/// and no folder outside it, finding `to` in the same way instead, as a rename of the
/// folder needs it. `folders` are all folders before the rename. A filter that also
/// found other folders stays as typed: it still means what it says.
pub fn rename_folder(query: &str, from: &str, to: &str, folders: &[String]) -> String {
    let inside = |folder: &str, root: &str| {
        folder
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    let after: Vec<String> = folders
        .iter()
        .map(|folder| match folder.strip_prefix(from) {
            Some(rest) if inside(folder, from) => format!("{to}{rest}"),
            _ => folder.clone(),
        })
        .collect();
    // Finds exactly the folders in `root` among `folders`.
    let finds_only = |value: &str, root: &str, folders: &[String]| {
        let mut found = folders
            .iter()
            .filter(|folder| folder_matches(value, folder))
            .peekable();
        found.peek().is_some() && found.all(|folder| inside(folder, root))
    };
    let mut renamed = String::with_capacity(query.len());
    let mut position = 0;
    for item in parse(query).items {
        let ItemKind::Filter(Filter::Folder(value)) = &item.kind else {
            continue;
        };
        if !finds_only(value, from, folders) {
            continue;
        }
        let Some(new) = renamed_value(value, from, to)
            .into_iter()
            .flatten()
            .flatten()
            .find(|new| finds_only(new, to, &after))
        else {
            continue;
        };
        renamed.push_str(&query[position..item.span.start]);
        renamed.push_str(&Filter::Folder(new).to_string());
        position = item.span.end;
    }
    renamed.push_str(&query[position..]);
    renamed
}

/// What the `folder:` value `value`, which names the folder `from` or one inside it,
/// could say once `from` is `to`: first in the form it was typed in, then as the
/// whole path from the notes folder. `None` if `value` does not name `from`.
fn renamed_value(value: &str, from: &str, to: &str) -> Option<[Option<String>; 2]> {
    let anchored = value.starts_with('/');
    let typed: Vec<&str> = value
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    let folded: Vec<String> = typed.iter().map(|segment| fold(segment).primary).collect();
    let old = folder_segments(from);
    let new: Vec<&str> = to.split('/').collect();
    // Where `value` starts in `from`: it covers the end of `from` and may go below.
    let start = (0..old.len()).find(|&start| {
        (!anchored || start == 0)
            && folded.len() >= old.len() - start
            && folded[..old.len() - start] == old[start..]
    })?;
    let below = typed[old.len() - start..]
        .iter()
        .map(|segment| format!("/{segment}"));
    let below: String = below.collect();
    let same_form = if start == 0 {
        Some(format!("{}{to}{below}", if anchored { "/" } else { "" }))
    } else {
        let tail = new.len().checked_sub(old.len() - start);
        tail.map(|tail| format!("{}{below}", new[tail..].join("/")))
    };
    Some([same_form, Some(format!("/{to}{below}"))])
}

/// Looks `value` up among the names of a filter's values: `Some(Some(_))` for a name,
/// `Some(None)` for the beginning of one (still being typed), `None` for anything else.
fn named<T: Copy>(names: &[(&str, T)], value: &str) -> Option<Option<T>> {
    let value = fold(value).primary;
    if let Some((_, found)) = names.iter().find(|(name, _)| *name == value) {
        Some(Some(*found))
    } else if names.iter().any(|(name, _)| name.starts_with(&value)) {
        Some(None)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_runs_list_every_run_of_segments() {
        assert_eq!(folder_runs(""), Vec::<String>::new());
        assert_eq!(folder_runs("Arbeit"), ["/arbeit", "arbeit"]);
        assert_eq!(
            folder_runs("Firma/Büro/Projekte"),
            [
                "/firma",
                "firma",
                "/firma/buro",
                "firma/buro",
                "/firma/buro/projekte",
                "firma/buro/projekte",
                "buro",
                "buro/projekte",
                "projekte",
            ]
        );
    }

    #[test]
    fn folder_keys_fold_and_keep_the_anchor() {
        assert_eq!(folder_key("Arbeit"), "arbeit");
        assert_eq!(folder_key("Büro/Projekte/"), "buro/projekte");
        assert_eq!(folder_key("/Meine Notizen"), "/meine notizen");
    }

    fn folders(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn a_folder_rename_follows_into_folder_filters() {
        let all = folders(&["Arbeit", "Arbeit/Kunden", "Arbeitszeit", "Privat"]);
        let rename = |query, from, to| rename_folder(query, from, to, &all);
        assert_eq!(
            rename("folder:/Arbeit open: plan", "Arbeit", "Büro"),
            "folder:/Büro open: plan"
        );
        assert_eq!(
            rename("folder:Arbeit/Kunden #x", "Arbeit", "Alt/Arbeit"),
            "folder:Alt/Arbeit/Kunden #x"
        );
        assert_eq!(
            rename("folder:/Arbeit", "Arbeit", "Neue Arbeit"),
            "folder:\"/Neue Arbeit\""
        );
        // Other folders, and words that only look alike, stay.
        assert_eq!(
            rename("folder:/Arbeitszeit Arbeit", "Arbeit", "Büro"),
            "folder:/Arbeitszeit Arbeit"
        );
        assert_eq!(rename("", "Arbeit", "Büro"), "");
    }

    #[test]
    fn a_folder_rename_follows_filters_as_the_search_matches_them() {
        let all = folders(&["Arbeit", "Arbeit/Kunden", "Privat"]);
        // Case and accents are folded, as in the search.
        assert_eq!(
            rename_folder("folder:arbeit open:", "Arbeit", "Büro", &all),
            "folder:Büro open:"
        );
        // Unanchored, a filter finds a folder at any depth.
        assert_eq!(
            rename_folder("folder:Kunden", "Arbeit/Kunden", "Arbeit/Clients", &all),
            "folder:Clients"
        );
        assert_eq!(
            rename_folder(
                "folder:/arbeit/kunden",
                "Arbeit/Kunden",
                "Privat/Leute",
                &all
            ),
            "folder:/Privat/Leute"
        );
    }

    #[test]
    fn a_filter_that_also_finds_other_folders_stays() {
        let all = folders(&["Arbeit", "Arbeit/Kunden", "Privat", "Privat/Kunden"]);
        assert_eq!(
            rename_folder("folder:Kunden", "Arbeit/Kunden", "Arbeit/Clients", &all),
            "folder:Kunden"
        );
        // Nor does a filter that finds no folder change.
        assert_eq!(
            rename_folder("folder:Fehlt", "Arbeit", "Büro", &all),
            "folder:Fehlt"
        );
    }

    #[test]
    fn a_filter_is_not_renamed_to_one_that_finds_more() {
        // "folder:Kunden" would also find Privat/Kunden after the rename.
        let all = folders(&["Arbeit", "Arbeit/Leute", "Privat", "Privat/Kunden"]);
        assert_eq!(
            rename_folder("folder:Leute", "Arbeit/Leute", "Arbeit/Kunden", &all),
            "folder:/Arbeit/Kunden"
        );
    }

    fn kinds(input: &str) -> Vec<ItemKind> {
        parse(input)
            .items
            .into_iter()
            .map(|item| item.kind)
            .collect()
    }

    fn word(text: &str) -> ItemKind {
        ItemKind::Word(text.to_owned())
    }

    fn phrase(text: &str) -> ItemKind {
        ItemKind::Phrase(text.to_owned())
    }

    fn pending(text: &str) -> ItemKind {
        ItemKind::Pending(text.to_owned())
    }

    fn tag(text: &str) -> ItemKind {
        ItemKind::Filter(Filter::Tag(text.to_owned()))
    }

    fn folder(text: &str) -> ItemKind {
        ItemKind::Filter(Filter::Folder(text.to_owned()))
    }

    #[test]
    fn words_and_phrases() {
        let cases = [
            ("", vec![]),
            ("   ", vec![]),
            ("milch", vec![word("milch")]),
            ("  Milch   Brot ", vec![word("Milch"), word("Brot")]),
            ("\"exact phrase\"", vec![phrase("exact phrase")]),
            (
                "vor \"exakte Phrase\" nach",
                vec![word("vor"), phrase("exakte Phrase"), word("nach")],
            ),
            ("\" padded \"", vec![phrase("padded")]),
            ("\"\"", vec![]),
            ("\"", vec![]),
            ("\"half typed", vec![phrase("half typed")]),
            ("\"a\"\"b\"", vec![phrase("a"), phrase("b")]),
            ("it\"s", vec![word("it\"s")]),
        ];
        for (input, expected) in cases {
            assert_eq!(kinds(input), expected, "{input:?}");
        }
    }

    #[test]
    fn filters_in_english_and_german() {
        let has = |feature| ItemKind::Filter(Filter::Has(feature));
        let modified = |period| ItemKind::Filter(Filter::Modified(period));
        let cases = [
            ("#rezepte", tag("rezepte")),
            ("#projekt/sheaf", tag("projekt/sheaf")),
            ("#projekt/", tag("projekt")),
            ("#Büro", tag("Büro")),
            ("folder:arbeit", folder("arbeit")),
            ("ordner:arbeit", folder("arbeit")),
            ("Ordner:Arbeit/Projekte", folder("Arbeit/Projekte")),
            ("folder:arbeit/", folder("arbeit")),
            ("folder:/arbeit", folder("/arbeit")),
            ("folder:\"Meine Notizen\"", folder("Meine Notizen")),
            ("ordner:\"Meine Notizen", folder("Meine Notizen")),
            ("has:checklist", has(Feature::Checklist)),
            ("hat:checkliste", has(Feature::Checklist)),
            ("HAT:Checkliste", has(Feature::Checklist)),
            ("has:image", has(Feature::Image)),
            ("hat:bild", has(Feature::Image)),
            ("has:table", has(Feature::Table)),
            ("hat:tabelle", has(Feature::Table)),
            ("has:attachment", has(Feature::Attachment)),
            ("hat:anhang", has(Feature::Attachment)),
            ("has:bild", has(Feature::Image)),
            ("open:", ItemKind::Filter(Filter::Open)),
            ("offen:", ItemKind::Filter(Filter::Open)),
            ("modified:today", modified(Period::Today)),
            ("modified:week", modified(Period::Week)),
            ("modified:month", modified(Period::Month)),
            ("geändert:heute", modified(Period::Today)),
            ("geändert:woche", modified(Period::Week)),
            ("geändert:monat", modified(Period::Month)),
            ("geaendert:woche", modified(Period::Week)),
            ("GEÄNDERT:Heute", modified(Period::Today)),
            ("pinned:", ItemKind::Filter(Filter::Pinned)),
            ("angeheftet:", ItemKind::Filter(Filter::Pinned)),
        ];
        for (input, expected) in cases {
            assert_eq!(kinds(input), vec![expected], "{input:?}");
        }
    }

    #[test]
    fn unknown_keys_and_invalid_values_are_words() {
        let cases = [
            "foo:bar",
            "https://example.org/a?b=c",
            "12:30",
            "has:unicorn",
            "hat:checklisten",
            "modified:yesterday",
            "open:yes",
            "pinned:no",
            "#!",
            "#-",
            "C#",
            "a#b",
            ":",
            ":value",
        ];
        for input in cases {
            assert_eq!(kinds(input), vec![word(input)], "{input:?}");
        }
    }

    #[test]
    fn half_typed_filters_are_pending() {
        let cases = [
            "#",
            "folder:",
            "ordner:",
            "folder:\"",
            "folder:\"\"",
            "folder:/",
            "has:",
            "has:ch",
            "hat:tab",
            "modified:",
            "geändert:he",
        ];
        for input in cases {
            assert_eq!(kinds(input), vec![pending(input)], "{input:?}");
        }
        assert_eq!(
            kinds("milch # brot folder:"),
            vec![
                word("milch"),
                pending("#"),
                word("brot"),
                pending("folder:")
            ]
        );
    }

    #[test]
    fn spans_cover_the_typed_text() {
        let input =
            "  Milch #einkauf/markt \"frische Äpfel\" ordner:\"Meine Notizen\" hat:bild offen: ha";
        let query = parse(input);
        let sources: Vec<&str> = query
            .items
            .iter()
            .map(|item| &input[item.span.clone()])
            .collect();
        assert_eq!(
            sources,
            [
                "Milch",
                "#einkauf/markt",
                "\"frische Äpfel\"",
                "ordner:\"Meine Notizen\"",
                "hat:bild",
                "offen:",
                "ha",
            ]
        );
        insta::assert_json_snapshot!(query);
    }

    #[test]
    fn span_of_an_unterminated_quote_reaches_the_end() {
        let input = "a \"b c ";
        let query = parse(input);
        assert_eq!(query.items[1].span, 2..input.len());
        assert_eq!(query.items[1].kind, phrase("b c"));
    }

    #[test]
    fn accessors_split_by_kind() {
        let query = parse("milch \"frisches brot\" #einkauf käse offen: has:");
        assert_eq!(query.words().collect::<Vec<_>>(), ["milch", "käse"]);
        assert_eq!(query.phrases().collect::<Vec<_>>(), ["frisches brot"]);
        assert_eq!(
            query.filters().collect::<Vec<_>>(),
            [&Filter::Tag("einkauf".to_owned()), &Filter::Open]
        );
    }

    #[test]
    fn display_is_canonical_english() {
        let cases = [
            ("  Milch   Brot ", "Milch Brot"),
            ("\" exakte Phrase \"", "\"exakte Phrase\""),
            ("\"half typed", "\"half typed\""),
            ("#Projekt/Sheaf/", "#Projekt/Sheaf"),
            ("ordner:arbeit", "folder:arbeit"),
            ("ordner:\"Meine Notizen", "folder:\"Meine Notizen\""),
            ("folder:\"arbeit\"", "folder:arbeit"),
            ("hat:checkliste", "has:checklist"),
            ("hat:bild", "has:image"),
            ("hat:tabelle", "has:table"),
            ("hat:anhang", "has:attachment"),
            ("offen:", "open:"),
            ("geändert:heute", "modified:today"),
            ("geaendert:woche", "modified:week"),
            ("GEÄNDERT:Monat", "modified:month"),
            ("angeheftet:", "pinned:"),
            ("hat:ch", "hat:ch"),
            ("foo:bar", "foo:bar"),
        ];
        for (input, expected) in cases {
            assert_eq!(parse(input).to_string(), expected, "{input:?}");
        }
    }

    #[test]
    fn display_parses_back_to_the_same_query() {
        let inputs = [
            "",
            "milch brot",
            "  Milch #einkauf/markt \"frische Äpfel\" ordner:\"Meine Notizen\" hat:bild offen: ha",
            "\"half typed",
            "ordner:\"halb getippt",
            "folder:\"a b\"c",
            "geändert:woche angeheftet: has:table hat:anhang modified:month",
            "# folder: has:ch foo:bar #! it\"s",
            "\"a\"\"b\" \"\" c",
            "folder:/arbeit/projekte/ #büro/",
        ];
        for input in inputs {
            let query = parse(input);
            let canonical = query.to_string();
            let reparsed = parse(&canonical);
            assert_eq!(
                reparsed
                    .items
                    .iter()
                    .map(|item| &item.kind)
                    .collect::<Vec<_>>(),
                query
                    .items
                    .iter()
                    .map(|item| &item.kind)
                    .collect::<Vec<_>>(),
                "{input:?} → {canonical:?}"
            );
            assert_eq!(reparsed.to_string(), canonical, "{input:?}");
        }
    }
}

//! A small hand-written notes folder and helpers shared by the search tests.

// Each test binary uses its own part of these helpers.
#![allow(dead_code, clippy::unwrap_used)]

use jiff::{Timestamp, ZonedDateTime};
use sheaf_core::{NoteDoc, NoteId, NoteMeta, SearchIndex, SearchResults};

/// The moment the tests pretend it is: a Monday noon in Central European Time.
pub fn now() -> ZonedDateTime {
    "2026-11-16T12:00:00+01:00[+01:00]".parse().unwrap()
}

/// Builds the note the storage layer would read from a file with this path and text.
pub fn note(rel_path: &str, body: &str) -> NoteDoc {
    let content: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let heading = |line: &str| {
        let text = line.trim_start_matches('#');
        (text.len() < line.len() && text.starts_with(' ')).then(|| text.trim().to_owned())
    };
    let title = content.first().map_or(String::new(), |line| {
        heading(line).unwrap_or_else(|| line.trim().to_owned())
    });
    let mut tags: Vec<String> = Vec::new();
    for word in body.split_whitespace() {
        let Some(tag) = word.strip_prefix('#') else {
            continue;
        };
        let known = tags.iter().any(|seen| seen.eq_ignore_ascii_case(tag));
        if tag.starts_with(char::is_alphabetic) && !known {
            tags.push(tag.to_owned());
        }
    }
    let links = body
        .split("[[")
        .skip(1)
        .filter_map(|rest| rest.split_once("]]"))
        .map(|(target, _)| target.split(['|', '#']).next().unwrap_or(target).to_owned())
        .collect();
    let modified: Timestamp = "2026-11-01T12:00:00Z".parse().unwrap();
    NoteDoc {
        meta: NoteMeta {
            id: NoteId::provisional(rel_path),
            rel_path: rel_path.to_owned(),
            folder: rel_path
                .rsplit_once('/')
                .map_or("", |(folder, _)| folder)
                .to_owned(),
            title,
            created: modified,
            modified,
            pinned: false,
            tags,
            tasks_open: body.matches("- [ ] ").count() as u32,
            tasks_total: (body.matches("- [ ] ").count() + body.matches("- [x] ").count()) as u32,
            has_image: body.contains("!["),
            has_table: body.lines().any(|line| line.starts_with('|')),
            has_attachment: body.contains(".pdf)"),
            conflict: rel_path.contains("sync-conflict"),
        },
        headings: content
            .iter()
            .skip(1)
            .filter_map(|line| heading(line))
            .collect(),
        body: body.to_owned(),
        links,
    }
}

pub trait NoteExt {
    fn modified(self, timestamp: &str) -> Self;
    fn created(self, timestamp: &str) -> Self;
    fn pinned(self) -> Self;
    fn tagged(self, tag: &str) -> Self;
}

impl NoteExt for NoteDoc {
    fn modified(mut self, timestamp: &str) -> Self {
        self.meta.modified = timestamp.parse().unwrap();
        self
    }

    fn created(mut self, timestamp: &str) -> Self {
        self.meta.created = timestamp.parse().unwrap();
        self
    }

    fn pinned(mut self) -> Self {
        self.meta.pinned = true;
        self
    }

    /// A tag from the frontmatter, which does not occur in the text.
    fn tagged(mut self, tag: &str) -> Self {
        self.meta.tags.push(tag.to_owned());
        self
    }
}

/// An in-memory index of `notes`.
pub fn index_of(notes: &[NoteDoc]) -> SearchIndex {
    let index = SearchIndex::in_memory().unwrap();
    let mut writer = index.writer().unwrap();
    for note in notes {
        writer.upsert(note).unwrap();
    }
    writer.commit().unwrap();
    index
}

pub fn titles(notes: &[NoteMeta]) -> Vec<&str> {
    notes.iter().map(|note| note.title.as_str()).collect()
}

pub fn hit_titles(results: &SearchResults) -> Vec<&str> {
    results
        .hits
        .iter()
        .map(|hit| hit.meta.title.as_str())
        .collect()
}

/// The notes folder most tests search. Times are relative to [`now`].
pub fn corpus() -> Vec<NoteDoc> {
    vec![
        note(
            "Arbeit/Ausschreibung Serverraum.md",
            "# Ausschreibung Serverraum\n\
             \n\
             Die Ausschreibung für den neuen Serverraum läuft bis Ende März.\n\
             \n\
             ## Anforderungen\n\
             - [ ] Angebote vergleichen\n\
             - [x] Leistungsverzeichnis schreiben\n\
             \n\
             | Anbieter | Preis |\n\
             |---|---|\n\
             | Nordkabel | 12.000 € |\n\
             \n\
             #arbeit/projekte\n",
        )
        .modified("2026-11-13T10:00:00Z"),
        note(
            "Privat/Einkäufe.md",
            "Einkäufe\n\
             \n\
             Milch, Brot und Käse für das Wochenende, dazu eine Flasche Wein und Tee.\n\
             - [ ] Milch\n\
             - [ ] Brötchen\n\
             \n\
             #einkauf\n",
        )
        .modified("2026-11-16T08:00:00Z"),
        note(
            "Privat/Rezepte/Apfelkuchen.md",
            "# Apfelkuchen\n\
             \n\
             ![Kuchen](_attachments/01/kuchen.jpg)\n\
             \n\
             ## Zutaten\n\
             - 500 g Äpfel\n\
             - 200 g Mehl\n\
             \n\
             ## Zubereitung\n\
             Den Teig kneten und die Äpfel schälen.\n\
             \n\
             #rezepte/backen\n",
        )
        .modified("2026-10-27T18:00:00Z"),
        note(
            "Arbeit/Projekte/Sheaf Roadmap.md",
            "# Sheaf Roadmap\n\
             \n\
             The search index must answer in under 30 ms. See [[Ausschreibung Serverraum]]\n\
             and the [design notes](_attachments/02/design.pdf).\n\
             \n\
             ## Milestones\n\
             - [x] storage\n\
             - [x] index\n\
             \n\
             #projekt/sheaf\n",
        )
        .modified("2026-09-10T09:00:00Z")
        .pinned(),
        note(
            "Meeting notes.md",
            "Meeting notes\n\
             \n\
             We discussed the shopping budget and the new espresso machine.\n\
             \n\
             #arbeit\n",
        )
        // Yesterday evening, local time: within 24 hours but not today.
        .modified("2026-11-15T22:30:00Z"),
        note(
            "Firma/Arbeit/Straßenfest.md",
            "Straßenfest\n\
             \n\
             Das Fest findet in der Hauptstraße statt. Größe des Zeltes: 5 m.\n",
        )
        .modified("2026-11-10T12:00:00Z"),
        note(
            "Meine Notizen/Reisen/Reise nach Zürich.md",
            "# Reise nach Zürich\n\
             \n\
             Abfahrt um 7:42 Uhr ab Gleis 3.\n",
        )
        .modified("2026-11-08T07:00:00Z"),
    ]
}

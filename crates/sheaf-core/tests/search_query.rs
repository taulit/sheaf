//! What a query finds, in which order, and what it highlights.

#![allow(clippy::unwrap_used)]

mod common;

use common::{NoteExt, corpus, hit_titles, index_of, note, now, titles};
use sheaf_core::query::parse;
use sheaf_core::snippet::SNIPPET_CHARS;
use sheaf_core::{Filter, Hit, NoteDoc, Period, Reader, SearchResults, Sort};

const AUSSCHREIBUNG: &str = "Ausschreibung Serverraum";
const EINKAUF: &str = "Einkäufe";
const KUCHEN: &str = "Apfelkuchen";
const ROADMAP: &str = "Sheaf Roadmap";
const MEETING: &str = "Meeting notes";
const FEST: &str = "Straßenfest";
const REISE: &str = "Reise nach Zürich";

fn search(reader: &Reader, query: &str) -> SearchResults {
    reader.search(&parse(query), 50, &now()).unwrap()
}

/// Asserts that every query finds exactly the expected notes, in any order.
fn assert_finds(reader: &Reader, cases: &[(&str, &[&str])]) {
    for (query, expected) in cases {
        let results = search(reader, query);
        let mut found = hit_titles(&results);
        found.sort_unstable();
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        assert_eq!(found, expected, "query {query:?}");
        assert_eq!(results.total, expected.len(), "total of {query:?}");
    }
}

/// Asserts that every query finds exactly the expected notes in this order.
fn assert_ranks(reader: &Reader, cases: &[(&str, &[&str])]) {
    for (query, expected) in cases {
        assert_eq!(
            hit_titles(&search(reader, query)),
            *expected,
            "query {query:?}"
        );
    }
}

#[test]
fn typos_are_tolerated_by_word_length() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("ausschriebung", &[AUSSCHREIBUNG, ROADMAP]),
            ("auschreibung", &[AUSSCHREIBUNG, ROADMAP]),
            ("serverruam", &[AUSSCHREIBUNG, ROADMAP]),
            // Three characters: no typos.
            ("tee", &[EINKAUF]),
            ("tea", &[]),
            ("the", &[ROADMAP, MEETING]),
            // Four to seven characters: one edit.
            ("brot", &[EINKAUF]),
            ("brod", &[EINKAUF]),
            ("bort", &[EINKAUF]),
            ("bxod", &[]),
            ("flasche", &[EINKAUF]),
            ("flaschr", &[EINKAUF]),
            ("flxschr", &[]),
            // From eight characters: two edits.
            ("espresso", &[MEETING]),
            ("esprexxo", &[MEETING]),
            ("expresso", &[MEETING]),
            ("exprexxo", &[]),
        ],
    );
}

#[test]
fn umlauts_and_sharp_s_match_in_every_spelling() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("einkäufe", &[EINKAUF]),
            ("einkaufe", &[EINKAUF]),
            ("einkaeufe", &[EINKAUF]),
            ("EINKÄUFE", &[EINKAUF]),
            ("einka\u{308}ufe", &[EINKAUF]),
            ("einkauf", &[EINKAUF]),
            ("käse", &[EINKAUF]),
            ("kase", &[EINKAUF]),
            ("kaese", &[EINKAUF]),
            ("äpfel", &[KUCHEN]),
            ("apfel", &[KUCHEN]),
            ("aepfel", &[KUCHEN]),
            ("zürich", &[REISE]),
            ("zurich", &[REISE]),
            ("zuerich", &[REISE]),
            ("straßenfest", &[FEST]),
            ("strassenfest", &[FEST]),
            ("hauptstraße", &[FEST]),
            ("hauptstrasse", &[FEST]),
            ("größe", &[FEST]),
            ("grosse", &[FEST]),
            ("groesse", &[FEST]),
            ("marz", &[AUSSCHREIBUNG]),
            ("maerz", &[AUSSCHREIBUNG]),
        ],
    );
}

#[test]
fn a_note_written_without_umlauts_is_found_with_them() {
    let notes = [
        note("a.md", "Liste\n\nEinkaeufe fuer die Woche"),
        note("b.md", "Zettel\n\nEinkaufe fur die Woche"),
    ];
    let reader = index_of(&notes).reader();
    assert_finds(
        &reader,
        &[
            ("einkäufe", &["Liste", "Zettel"]),
            ("\"einkäufe für\"", &["Liste", "Zettel"]),
            ("für", &["Liste", "Zettel"]),
        ],
    );
}

#[test]
fn prefixes_match_in_title_headings_and_body() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            // Title.
            ("serverr", &[AUSSCHREIBUNG, ROADMAP]),
            ("apfelk", &[KUCHEN]),
            ("str", &[FEST]),
            ("ro", &[ROADMAP]),
            // Longer than the n-grams reach.
            ("ausschreibung", &[AUSSCHREIBUNG, ROADMAP]),
            // Headings.
            ("anford", &[AUSSCHREIBUNG]),
            ("zuber", &[KUCHEN]),
            ("mile", &[ROADMAP]),
            // Body.
            ("ausschr", &[AUSSCHREIBUNG, ROADMAP]),
            ("wochenen", &[EINKAUF]),
            ("leistungsverz", &[AUSSCHREIBUNG]),
            ("einkauf", &[EINKAUF]),
            ("sho", &[MEETING]),
            // Tags.
            ("back", &[KUCHEN]),
            // A single character.
            ("z", &[KUCHEN, FEST, REISE]),
            ("q", &[]),
        ],
    );
}

#[test]
fn every_word_is_required() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("milch brot", &[EINKAUF]),
            ("brot milch käse wein", &[EINKAUF]),
            ("milch serverraum", &[]),
            ("ausschreibung index", &[ROADMAP]),
            ("budget espr", &[MEETING]),
            // Split by the analysis: both parts are required, only the last completes.
            ("12.000", &[AUSSCHREIBUNG]),
            ("7:4", &[REISE]),
            // Nothing to search for.
            (
                "&",
                &[
                    AUSSCHREIBUNG,
                    EINKAUF,
                    KUCHEN,
                    ROADMAP,
                    MEETING,
                    FEST,
                    REISE,
                ],
            ),
        ],
    );
}

#[test]
fn phrases_match_in_order_and_without_typos() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("\"neuen Serverraum\"", &[AUSSCHREIBUNG]),
            ("\"Serverraum neuen\"", &[]),
            ("\"neuen Serverr\"", &[]),
            ("\"neuen Serverruam\"", &[]),
            ("\"ausschreibung serverraum\"", &[AUSSCHREIBUNG, ROADMAP]),
            ("\"die äpfel schälen\"", &[KUCHEN]),
            ("\"die aepfel schaelen\"", &[KUCHEN]),
            ("\"die apfel schalen\"", &[KUCHEN]),
            ("\"einkäufe\"", &[EINKAUF]),
            ("\"einkauf\"", &[EINKAUF]),
            ("\"einka\"", &[]),
            ("\"shopping budget\" espresso", &[MEETING]),
            ("\"shopping budget\" milch", &[]),
            ("\"reise nach\" \"gleis 3\"", &[REISE]),
            // Across punctuation and Markdown.
            ("\"wochenende dazu eine flasche\"", &[EINKAUF]),
            ("\"anbieter preis\"", &[AUSSCHREIBUNG]),
            ("\"half typed sho", &[]),
            ("\"shopping bud", &[]),
            ("\"shopping budget", &[MEETING]),
        ],
    );
}

#[test]
fn filters_restrict_in_german_and_english() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("#arbeit", &[AUSSCHREIBUNG, MEETING]),
            ("#ARBEIT", &[AUSSCHREIBUNG, MEETING]),
            ("#arbeit/projekte", &[AUSSCHREIBUNG]),
            ("#projekte", &[]),
            ("#rezepte", &[KUCHEN]),
            ("#rezepte/backen", &[KUCHEN]),
            ("#rezepte/kochen", &[]),
            ("#projekt", &[ROADMAP]),
            ("#arb", &[]),
            ("folder:arbeit", &[AUSSCHREIBUNG, ROADMAP, FEST]),
            ("ordner:arbeit", &[AUSSCHREIBUNG, ROADMAP, FEST]),
            ("Ordner:ARBEIT", &[AUSSCHREIBUNG, ROADMAP, FEST]),
            ("folder:arbeit/projekte", &[ROADMAP]),
            ("folder:projekte", &[ROADMAP]),
            ("folder:firma", &[FEST]),
            ("folder:firma/arbeit", &[FEST]),
            ("folder:/arbeit", &[AUSSCHREIBUNG, ROADMAP]),
            ("folder:/firma/arbeit", &[FEST]),
            ("folder:arbeit/firma", &[]),
            ("folder:arb", &[]),
            ("folder:privat", &[EINKAUF, KUCHEN]),
            ("folder:rezepte", &[KUCHEN]),
            ("folder:\"Meine Notizen\"", &[REISE]),
            ("ordner:\"meine notizen/reisen\"", &[REISE]),
            ("folder:reisen", &[REISE]),
            ("has:checklist", &[AUSSCHREIBUNG, EINKAUF, ROADMAP]),
            ("hat:checkliste", &[AUSSCHREIBUNG, EINKAUF, ROADMAP]),
            ("has:image", &[KUCHEN]),
            ("hat:bild", &[KUCHEN]),
            ("has:table", &[AUSSCHREIBUNG]),
            ("hat:tabelle", &[AUSSCHREIBUNG]),
            ("has:attachment", &[ROADMAP]),
            ("hat:anhang", &[ROADMAP]),
            ("open:", &[AUSSCHREIBUNG, EINKAUF]),
            ("offen:", &[AUSSCHREIBUNG, EINKAUF]),
            ("pinned:", &[ROADMAP]),
            ("angeheftet:", &[ROADMAP]),
            ("modified:today", &[EINKAUF]),
            ("geändert:heute", &[EINKAUF]),
            ("modified:week", &[EINKAUF, MEETING, AUSSCHREIBUNG, FEST]),
            ("geändert:woche", &[EINKAUF, MEETING, AUSSCHREIBUNG, FEST]),
            ("geaendert:woche", &[EINKAUF, MEETING, AUSSCHREIBUNG, FEST]),
            (
                "modified:month",
                &[EINKAUF, MEETING, AUSSCHREIBUNG, FEST, REISE, KUCHEN],
            ),
            (
                "geändert:monat",
                &[EINKAUF, MEETING, AUSSCHREIBUNG, FEST, REISE, KUCHEN],
            ),
            // Several filters all apply.
            ("folder:arbeit has:checklist", &[AUSSCHREIBUNG, ROADMAP]),
            ("folder:arbeit hat:checkliste offen:", &[AUSSCHREIBUNG]),
            ("#arbeit modified:today", &[]),
        ],
    );
}

#[test]
fn filters_combine_with_words_and_phrases() {
    let reader = index_of(&corpus()).reader();
    assert_finds(
        &reader,
        &[
            ("#arbeit ausschreibung", &[AUSSCHREIBUNG]),
            ("ausschreibung #arbeit", &[AUSSCHREIBUNG]),
            ("ordner:privat milch", &[EINKAUF]),
            ("ordner:arbeit milch", &[]),
            ("hat:checkliste index", &[ROADMAP]),
            ("offen: serverr", &[AUSSCHREIBUNG]),
            ("geändert:woche strasse", &[FEST]),
            ("pinned: \"search index\"", &[ROADMAP]),
            ("folder:projekte ausschriebung", &[ROADMAP]),
            // Filters that are still being typed do not restrict yet.
            ("milch folder:", &[EINKAUF]),
            ("milch has:ch", &[EINKAUF]),
            ("milch #", &[EINKAUF]),
            // An invalid value makes it words like any other.
            ("milch has:unicorn", &[]),
        ],
    );
}

#[test]
fn queries_without_words_list_newest_first() {
    let reader = index_of(&corpus()).reader();
    assert_ranks(
        &reader,
        &[
            ("folder:arbeit", &[AUSSCHREIBUNG, FEST, ROADMAP]),
            ("has:checklist", &[EINKAUF, AUSSCHREIBUNG, ROADMAP]),
            (
                "",
                &[
                    EINKAUF,
                    MEETING,
                    AUSSCHREIBUNG,
                    FEST,
                    REISE,
                    KUCHEN,
                    ROADMAP,
                ],
            ),
            (
                "#",
                &[
                    EINKAUF,
                    MEETING,
                    AUSSCHREIBUNG,
                    FEST,
                    REISE,
                    KUCHEN,
                    ROADMAP,
                ],
            ),
        ],
    );
    let recent = reader.recent(50).unwrap();
    assert_eq!(hit_titles(&search(&reader, "")), titles(&recent));
    assert!(search(&reader, "").hits.iter().all(|hit| hit.score == 0.0));
}

#[test]
fn the_limit_cuts_hits_but_not_the_total() {
    let reader = index_of(&corpus()).reader();
    for query in ["", "folder:arbeit", "e"] {
        let all = search(&reader, query);
        let cut = reader.search(&parse(query), 2, &now()).unwrap();
        assert_eq!(cut.hits, all.hits[..2], "query {query:?}");
        assert_eq!(cut.total, all.total, "query {query:?}");
        assert!(all.total > 2, "query {query:?}");
        let none = reader.search(&parse(query), 0, &now()).unwrap();
        assert!(none.hits.is_empty());
        assert_eq!(none.total, all.total);
    }
}

#[test]
fn listing_filters_and_sorts() {
    let notes: Vec<NoteDoc> = corpus()
        .into_iter()
        .enumerate()
        .map(|(index, note)| note.created(&format!("2026-01-{:02}T00:00:00Z", 20 - index)))
        .collect();
    let reader = index_of(&notes).reader();
    let list = |filters: &[Filter], sort, limit| {
        let notes = reader.list(filters, sort, limit, &now()).unwrap();
        notes.into_iter().map(|note| note.title).collect::<Vec<_>>()
    };
    assert_eq!(
        list(&[], Sort::Modified, None),
        [
            EINKAUF,
            MEETING,
            AUSSCHREIBUNG,
            FEST,
            REISE,
            KUCHEN,
            ROADMAP
        ]
    );
    assert_eq!(
        list(&[], Sort::Created, None),
        [
            AUSSCHREIBUNG,
            EINKAUF,
            KUCHEN,
            ROADMAP,
            MEETING,
            FEST,
            REISE
        ]
    );
    assert_eq!(
        list(&[], Sort::Title, None),
        [
            KUCHEN,
            AUSSCHREIBUNG,
            EINKAUF,
            MEETING,
            REISE,
            ROADMAP,
            FEST
        ]
    );
    assert_eq!(
        list(&[], Sort::Title, Some(3)),
        [KUCHEN, AUSSCHREIBUNG, EINKAUF]
    );
    assert_eq!(list(&[], Sort::Modified, Some(2)), [EINKAUF, MEETING]);
    assert_eq!(
        list(&[Filter::Folder("arbeit".into())], Sort::Title, None),
        [AUSSCHREIBUNG, ROADMAP, FEST]
    );
    assert_eq!(
        list(
            &[Filter::Modified(Period::Week), Filter::Open],
            Sort::Modified,
            None
        ),
        [EINKAUF, AUSSCHREIBUNG]
    );
    assert_eq!(
        list(&[Filter::Tag("nix".into())], Sort::Modified, None),
        [""; 0]
    );
}

#[test]
fn today_starts_at_local_midnight() {
    let reader = index_of(&corpus()).reader();
    let today = |now: &str| {
        let now = now.parse().unwrap();
        let notes = reader.list(
            &[Filter::Modified(Period::Today)],
            Sort::Modified,
            None,
            &now,
        );
        notes
            .unwrap()
            .into_iter()
            .map(|note| note.title)
            .collect::<Vec<_>>()
    };
    // The meeting note was changed at 22:30 UTC on the 15th: yesterday in Berlin, but
    // already today two hours further east, and today again by UTC's calendar.
    assert_eq!(today("2026-11-16T12:00:00+01:00[+01:00]"), [EINKAUF]);
    assert_eq!(
        today("2026-11-16T12:00:00+03:00[+03:00]"),
        [EINKAUF, MEETING]
    );
    assert_eq!(today("2026-11-15T23:59:00+00:00[UTC]"), [EINKAUF, MEETING]);
    assert_eq!(today("2026-11-17T00:00:00+01:00[+01:00]"), [""; 0]);
}

/// Notes that each have the word "kolibri" in exactly one place.
fn kolibri_notes() -> Vec<NoteDoc> {
    vec![
        note("body.md", "Vögel\n\nIm Garten war heute ein Kolibri."),
        note(
            "heading.md",
            "Garten\n\n## Der Kolibri\n\nEr trinkt Nektar.",
        ),
        note("tag.md", "Tierfotos\n\nBilder vom Wochenende.").tagged("kolibri"),
        note("title.md", "Der Kolibri im Garten\n\nNotizen folgen."),
    ]
}

#[test]
fn title_outranks_tags_outrank_headings_outrank_body() {
    let expected = ["Der Kolibri im Garten", "Tierfotos", "Garten", "Vögel"];
    // Whatever the notes' age: the weakest match is the newest in the first round and
    // the oldest in the second.
    for newest_first in [true, false] {
        let notes: Vec<NoteDoc> = kolibri_notes()
            .into_iter()
            .enumerate()
            .map(|(index, note)| {
                let day = if newest_first { 20 - index } else { 10 + index };
                note.modified(&format!("2026-11-{day:02}T00:00:00Z"))
            })
            .collect();
        let reader = index_of(&notes).reader();
        for query in ["kolibri", "kolib", "kolibir", "KOLIBRI"] {
            assert_eq!(
                hit_titles(&search(&reader, query)),
                expected,
                "query {query:?}"
            );
        }
    }
}

#[test]
fn a_tag_in_the_text_does_not_outrank_a_title() {
    let notes = [
        note("a.md", "Fotos\n\nBilder #kolibri").modified("2026-11-15T00:00:00Z"),
        note("b.md", "Kolibri füttern\n\nZuckerwasser.").modified("2026-11-01T00:00:00Z"),
    ];
    let reader = index_of(&notes).reader();
    assert_ranks(&reader, &[("kolibri", &["Kolibri füttern", "Fotos"])]);
}

#[test]
fn exact_outranks_prefix_outranks_fuzzy() {
    let in_body = [
        note("fuzzy.md", "Tiere\n\nEine Maus im Keller.").modified("2026-11-15T00:00:00Z"),
        note("prefix.md", "Reparatur\n\nDie Haustür klemmt.").modified("2026-11-14T00:00:00Z"),
        note("exact.md", "Umzug\n\nDas Haus ist leer.").modified("2026-11-13T00:00:00Z"),
    ];
    let reader = index_of(&in_body).reader();
    assert_ranks(&reader, &[("haus", &["Umzug", "Reparatur", "Tiere"])]);

    let in_title = [
        note("fuzzy.md", "Maus gesehen\n\nIm Keller.").modified("2026-11-15T00:00:00Z"),
        note("prefix.md", "Haustür klemmt\n\nSchloss ölen.").modified("2026-11-14T00:00:00Z"),
        note("exact.md", "Das Haus\n\nIst leer.").modified("2026-11-13T00:00:00Z"),
    ];
    let reader = index_of(&in_title).reader();
    let results = search(&reader, "haus");
    assert_eq!(
        hit_titles(&results),
        ["Das Haus", "Haustür klemmt", "Maus gesehen"]
    );
    let scores: Vec<f32> = results.hits.iter().map(|hit| hit.score).collect();
    assert!(scores[0] > scores[1] && scores[1] > scores[2], "{scores:?}");
}

#[test]
fn an_exact_word_in_the_text_outranks_a_typo_match_in_a_title() {
    let notes = [
        note("a.md", "Maus gesehen\n\nIm Keller.").modified("2026-11-15T00:00:00Z"),
        note("b.md", "Umzug\n\nDas Haus ist leer.").modified("2026-11-01T00:00:00Z"),
    ];
    let reader = index_of(&notes).reader();
    assert_ranks(&reader, &[("haus", &["Umzug", "Maus gesehen"])]);
}

#[test]
fn equal_matches_are_ordered_by_modification_time() {
    let notes = [
        note("a.md", "Montag\n\nMilch kaufen.").modified("2026-11-09T08:00:00Z"),
        note("b.md", "Mittwoch\n\nMilch kaufen und Brot.").modified("2026-11-11T08:00:00Z"),
        note("c.md", "Dienstag\n\nKeine Milch mehr, Milch, Milch!")
            .modified("2026-11-10T08:00:00Z"),
    ];
    let reader = index_of(&notes).reader();
    let results = search(&reader, "milch");
    assert_eq!(hit_titles(&results), ["Mittwoch", "Dienstag", "Montag"]);
    assert!(
        results
            .hits
            .iter()
            .all(|hit| hit.score == results.hits[0].score)
    );
    assert_ranks(
        &reader,
        &[
            ("milc", &["Mittwoch", "Dienstag", "Montag"]),
            ("mlich", &["Mittwoch", "Dienstag", "Montag"]),
            ("\"milch\"", &["Mittwoch", "Dienstag", "Montag"]),
            ("milch kaufen", &["Mittwoch", "Montag"]),
        ],
    );
}

#[test]
fn a_title_that_is_the_query_comes_first() {
    let notes = [
        note("a.md", "Einkauf für die Party\n\nEinkauf, Einkauf.").modified("2026-11-15T00:00:00Z"),
        note("b.md", "Einkauf\n\nMilch.").modified("2026-11-01T00:00:00Z"),
        note("c.md", "Großer Einkauf\n\nAlles.").modified("2026-11-14T00:00:00Z"),
        note("d.md", "Einkäufe\n\nNoch mehr.").modified("2026-11-16T00:00:00Z"),
    ];
    let reader = index_of(&notes).reader();
    assert_ranks(
        &reader,
        &[
            (
                "einkauf",
                &[
                    "Einkauf",
                    "Einkauf für die Party",
                    "Großer Einkauf",
                    "Einkäufe",
                ],
            ),
            (
                "EINKAUF",
                &[
                    "Einkauf",
                    "Einkauf für die Party",
                    "Großer Einkauf",
                    "Einkäufe",
                ],
            ),
            (
                "einkäufe",
                &[
                    "Einkäufe",
                    "Einkauf für die Party",
                    "Großer Einkauf",
                    "Einkauf",
                ],
            ),
            (
                "einkaeufe",
                &[
                    "Einkäufe",
                    "Einkauf für die Party",
                    "Großer Einkauf",
                    "Einkauf",
                ],
            ),
            ("grosser einkauf", &["Großer Einkauf"]),
            ("\"einkauf für die party\"", &["Einkauf für die Party"]),
            ("party einkauf", &["Einkauf für die Party"]),
        ],
    );
}

fn only_hit(reader: &Reader, query: &str) -> Hit {
    let mut results = search(reader, query);
    assert_eq!(
        results.hits.len(),
        1,
        "query {query:?}: {:?}",
        hit_titles(&results)
    );
    results.hits.remove(0)
}

fn highlighted(hit: &Hit) -> Vec<&str> {
    hit.snippet
        .highlights
        .iter()
        .map(|range| &hit.snippet.text[range.clone()])
        .collect()
}

fn title_highlighted(hit: &Hit) -> Vec<&str> {
    hit.title_highlights
        .iter()
        .map(|range| &hit.meta.title[range.clone()])
        .collect()
}

#[test]
fn snippets_highlight_the_word_in_the_note_not_the_query() {
    let reader = index_of(&corpus()).reader();
    let cases: [(&str, &[&str], &[&str]); 12] = [
        ("brot", &[], &["Brot", "Brötchen"]),
        ("wochenen", &[], &["Wochenende"]),
        ("brod", &[], &["Brot"]),
        ("flaschr", &[], &["Flasche"]),
        ("kaese", &[], &["Käse"]),
        ("kase", &[], &["Käse"]),
        ("KÄSE", &[], &["Käse"]),
        ("einkauf", &["Einkäufe"], &["einkauf"]),
        ("einkaeufe", &["Einkäufe"], &["einkauf"]),
        ("milch wein", &[], &["Milch", "Wein", "Milch"]),
        ("\"flasche wein\"", &[], &["Flasche", "Wein"]),
        ("\"brot und\" tee", &[], &["Brot", "und", "Tee"]),
    ];
    for (query, in_title, in_snippet) in cases {
        let hit = only_hit(&reader, query);
        assert_eq!(hit.meta.title, EINKAUF, "query {query:?}");
        assert_eq!(title_highlighted(&hit), in_title, "title of {query:?}");
        assert_eq!(highlighted(&hit), in_snippet, "snippet of {query:?}");
    }
}

#[test]
fn snippets_are_plain_text_without_the_title_line() {
    let reader = index_of(&corpus()).reader();

    let hit = only_hit(&reader, "leistungsverz");
    assert_eq!(
        hit.snippet.text,
        "…den neuen Serverraum läuft bis Ende März. Anforderungen Angebote vergleichen \
         Leistungsverzeichnis schreiben Anbieter Preis Nordkabel 12.000 € #arbeit/projekte"
    );
    assert_eq!(highlighted(&hit), ["Leistungsverzeichnis"]);
    assert!(hit.title_highlights.is_empty());

    let hit = only_hit(&reader, "design");
    assert_eq!(
        hit.snippet.text,
        "The search index must answer in under 30 ms. See Ausschreibung Serverraum and \
         the design notes. Milestones storage index #projekt/sheaf"
    );
    assert_eq!(highlighted(&hit), ["design"]);
}

#[test]
fn a_title_only_match_shows_the_beginning_of_the_text() {
    let reader = index_of(&corpus()).reader();
    let hit = only_hit(&reader, "apfelkuchen");
    assert_eq!(title_highlighted(&hit), ["Apfelkuchen"]);
    assert_eq!(
        hit.snippet.text,
        "Kuchen Zutaten 500 g Äpfel 200 g Mehl Zubereitung Den Teig kneten und die Äpfel \
         schälen. #rezepte/backen"
    );
    assert!(hit.snippet.highlights.is_empty());

    let hit = only_hit(&reader, "reise zürich");
    assert_eq!(title_highlighted(&hit), ["Reise", "Zürich"]);
    assert_eq!(hit.snippet.text, "Abfahrt um 7:42 Uhr ab Gleis 3.");

    let hit = only_hit(
        &index_of(&[note("a.md", "Nur ein Titel")]).reader(),
        "titel",
    );
    assert_eq!(hit.snippet.text, "");
}

#[test]
fn filter_only_hits_show_the_beginning_of_the_text() {
    let reader = index_of(&corpus()).reader();
    let hit = only_hit(&reader, "has:image");
    assert!(hit.snippet.text.starts_with("Kuchen Zutaten 500 g Äpfel"));
    assert!(hit.snippet.highlights.is_empty());
    assert!(hit.title_highlights.is_empty());
}

/// A long note with one early mention of "Zebra" and, far apart from it, a place where
/// "Zebra" and "Giraffe" stand together.
fn long_note() -> NoteDoc {
    let filler = "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod. ";
    let body = format!(
        "Zoobesuch\n\nAm Eingang stand ein Zebra. {}Später sahen wir die Giraffe neben \
         dem Zebra am Wasserloch stehen. {}Ende des Berichts.",
        filler.repeat(12),
        filler.repeat(12),
    );
    note("zoo.md", &body)
}

#[test]
fn a_long_note_is_cut_around_the_densest_matches() {
    let reader = index_of(&[long_note()]).reader();
    let hit = only_hit(&reader, "zebra giraffe");
    let snippet = &hit.snippet;
    assert_eq!(highlighted(&hit), ["Giraffe", "Zebra"]);
    assert!(
        snippet.text.starts_with('…') && snippet.text.ends_with('…'),
        "{snippet:?}"
    );
    let inner = snippet.text.trim_matches('…');
    assert!(inner.chars().count() <= SNIPPET_CHARS, "{snippet:?}");
    assert!(inner.chars().count() > SNIPPET_CHARS - 20, "{snippet:?}");
    // Cut between words: the plain text contains the snippet framed by spaces.
    let plain = sheaf_core::snippet::plain_text(&long_note().body);
    assert!(plain.contains(&format!(" {inner} ")), "{snippet:?}");
    // The matches sit in the middle.
    let first = snippet.highlights[0].start;
    let last = snippet.highlights[1].end;
    let trailing = snippet.text.len() - last;
    assert!(first.abs_diff(trailing) < 24, "{snippet:?}");

    let hit = only_hit(&reader, "zebra");
    assert_eq!(highlighted(&hit), ["Zebra"]);
    assert!(
        hit.snippet
            .text
            .starts_with("Am Eingang stand ein Zebra. Lorem")
    );
    assert!(hit.snippet.text.ends_with('…'));

    let hit = only_hit(&reader, "berichts");
    assert!(hit.snippet.text.starts_with('…'));
    assert!(hit.snippet.text.ends_with("Ende des Berichts."));
    assert_eq!(highlighted(&hit), ["Berichts"]);
}

#[test]
fn highlight_ranges_are_valid_for_every_hit() {
    let mut notes = corpus();
    notes.push(long_note());
    notes.push(note(
        "kyrillisch.md",
        "Привет\n\nПривет мир, это заметка о мире.",
    ));
    notes.push(note(
        "emoji.md",
        "🎉 Feier\n\n🎂 Kuchen für Jürgen – „süß“ und saftig…",
    ));
    let reader = index_of(&notes).reader();
    let queries = [
        "a",
        "e",
        "s",
        "de",
        "ein",
        "kuchen",
        "sus",
        "jurgen",
        "привет",
        "мир",
        "zebra lorem",
        "\"lorem ipsum\"",
        "m",
        "und",
        "feier",
    ];
    for query in queries {
        let results = search(&reader, query);
        assert!(!results.hits.is_empty(), "query {query:?}");
        for hit in &results.hits {
            let title = &hit.meta.title;
            let snippet = &hit.snippet;
            for (text, ranges) in [
                (title, &hit.title_highlights),
                (&snippet.text, &snippet.highlights),
            ] {
                let mut end = 0;
                for range in ranges {
                    assert!(
                        range.start >= end && range.start < range.end,
                        "{query:?} in {text:?}"
                    );
                    assert!(text.get(range.clone()).is_some(), "{query:?} in {text:?}");
                    assert!(
                        text[range.clone()].chars().all(char::is_alphanumeric),
                        "{query:?} highlights {:?}",
                        &text[range.clone()]
                    );
                    end = range.end;
                }
            }
            assert!(
                !hit.title_highlights.is_empty() || !snippet.highlights.is_empty(),
                "{query:?} highlights nothing in {title:?}"
            );
            assert!(
                snippet.text.chars().count() <= SNIPPET_CHARS + 2,
                "{snippet:?}"
            );
            assert!(!snippet.text.contains('\n'));
        }
    }
}

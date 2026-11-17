//! The life of an index: writing, replacing, reopening, and what readers can list.

#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use common::{NoteExt, corpus, index_of, note, now, titles};
use sheaf_core::query::parse;
use sheaf_core::{Error, NoteId, SearchIndex, Sort};

#[test]
fn every_field_of_a_note_comes_back_from_the_index() {
    let notes = corpus();
    let reader = index_of(&notes).reader();
    assert_eq!(reader.count(), notes.len());
    for note in &notes {
        let meta = &note.meta;
        assert_eq!(reader.get(&meta.id).unwrap().as_ref(), Some(meta));
        assert_eq!(
            reader.get_by_path(&meta.rel_path).unwrap().as_ref(),
            Some(meta)
        );
        assert_eq!(
            reader.body(&meta.id).unwrap().as_deref(),
            Some(note.body.as_str())
        );
    }
    let listed = reader.list(&[], Sort::Title, None, &now()).unwrap();
    assert!(notes.iter().all(|note| listed.contains(&note.meta)));

    let missing = NoteId::provisional("nope.md");
    assert_eq!(reader.get(&missing).unwrap(), None);
    assert_eq!(reader.get_by_path("nope.md").unwrap(), None);
    assert_eq!(reader.body(&missing).unwrap(), None);
}

#[test]
fn timestamps_keep_their_nanoseconds() {
    let note = note("a.md", "Genau").modified("2026-11-16T10:12:13.123456789Z");
    let reader = index_of(std::slice::from_ref(&note)).reader();
    assert_eq!(
        reader.get(&note.meta.id).unwrap().unwrap().modified,
        note.meta.modified
    );
    assert_eq!(reader.manifest().unwrap()[0].2, note.meta.modified);
}

#[test]
fn upsert_replaces_the_note_at_the_same_path() {
    let index = index_of(&corpus());
    let reader = index.reader();
    let mut writer = index.writer().unwrap();

    let replacement =
        note("Privat/Einkäufe.md", "Einkäufe\n\nNur noch Kaffee.").modified("2026-11-16T11:00:00Z");
    writer.upsert(&replacement).unwrap();
    writer.commit().unwrap();

    assert_eq!(reader.count(), corpus().len());
    assert_eq!(
        reader.get_by_path("Privat/Einkäufe.md").unwrap(),
        Some(replacement.meta.clone())
    );
    let find = |query| reader.search(&parse(query), 10, &now()).unwrap().total;
    assert_eq!(find("kaffee"), 1);
    assert_eq!(find("milch"), 0);
    assert_eq!(find("open:"), 1);
    assert_eq!(
        reader
            .tags()
            .unwrap()
            .iter()
            .filter(|(tag, _)| tag == "einkauf")
            .count(),
        0
    );
}

#[test]
fn a_moved_note_is_removed_and_added() {
    let index = index_of(&corpus());
    let reader = index.reader();
    let mut writer = index.writer().unwrap();
    let moved = note("Archiv/Meeting notes.md", "Meeting notes\n\nArchived.");
    writer.remove_path("Meeting notes.md");
    writer.upsert(&moved).unwrap();
    writer.commit().unwrap();

    assert_eq!(reader.count(), corpus().len());
    assert_eq!(reader.get_by_path("Meeting notes.md").unwrap(), None);
    assert_eq!(
        reader.get_by_path("Archiv/Meeting notes.md").unwrap(),
        Some(moved.meta)
    );
    assert_eq!(
        reader.search(&parse("espresso"), 10, &now()).unwrap().total,
        0
    );
}

#[test]
fn remove_and_clear() {
    let index = index_of(&corpus());
    let reader = index.reader();
    let mut writer = index.writer().unwrap();

    writer.remove_path("Privat/Einkäufe.md");
    writer.remove_path("does/not/exist.md");
    writer.commit().unwrap();
    assert_eq!(reader.count(), corpus().len() - 1);
    assert_eq!(reader.get_by_path("Privat/Einkäufe.md").unwrap(), None);
    assert_eq!(reader.search(&parse("milch"), 10, &now()).unwrap().total, 0);
    assert!(
        reader
            .recent(50)
            .unwrap()
            .iter()
            .all(|note| note.title != "Einkäufe")
    );
    assert!(
        reader
            .manifest()
            .unwrap()
            .iter()
            .all(|(path, ..)| path != "Privat/Einkäufe.md")
    );
    assert!(
        reader
            .folders()
            .unwrap()
            .contains(&("Privat/Rezepte".to_owned(), 1))
    );
    assert!(
        !reader
            .folders()
            .unwrap()
            .iter()
            .any(|(folder, _)| folder == "Privat")
    );

    writer.clear().unwrap();
    writer.commit().unwrap();
    assert_eq!(reader.count(), 0);
    assert!(reader.recent(50).unwrap().is_empty());
    assert!(reader.manifest().unwrap().is_empty());
    assert!(reader.tags().unwrap().is_empty());
    assert!(reader.folders().unwrap().is_empty());
    assert_eq!(
        reader
            .search(&parse("ausschreibung"), 10, &now())
            .unwrap()
            .total,
        0
    );

    writer.upsert(&note("neu.md", "Neu\n\nWieder da.")).unwrap();
    writer.commit().unwrap();
    assert_eq!(titles(&reader.recent(50).unwrap()), ["Neu"]);
}

#[test]
fn nothing_is_visible_before_the_commit() {
    let index = SearchIndex::in_memory().unwrap();
    let reader = index.reader();
    let mut writer = index.writer().unwrap();
    writer.upsert(&note("a.md", "Erste")).unwrap();
    assert_eq!(reader.count(), 0);
    writer.commit().unwrap();
    assert_eq!(reader.count(), 1);

    // Dropping the writer discards what was not committed.
    writer.upsert(&note("b.md", "Zweite")).unwrap();
    drop(writer);
    reader.reload().unwrap();
    assert_eq!(reader.count(), 1);
}

#[test]
fn there_is_one_writer_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let index = SearchIndex::open_or_create(dir.path()).unwrap();
    let writer = index.writer().unwrap();
    let error = index.writer().unwrap_err();
    assert!(
        matches!(&error, Error::Index(message) if message.contains("locked")),
        "{error}"
    );

    // Also across handles, as between two processes.
    let other = SearchIndex::open_or_create(dir.path()).unwrap();
    assert!(matches!(other.writer(), Err(Error::Index(_))));

    writer.finish().unwrap();
    other.writer().unwrap().finish().unwrap();
}

#[test]
fn the_writer_can_be_dropped_between_bursts() {
    let dir = tempfile::tempdir().unwrap();
    let index = SearchIndex::open_or_create(dir.path()).unwrap();
    let reader = index.reader();
    for (round, note) in corpus().into_iter().enumerate() {
        let mut writer = index.writer().unwrap();
        writer.upsert(&note).unwrap();
        writer.finish().unwrap();
        assert_eq!(reader.count(), round + 1);
    }
    drop(index);

    let reopened = SearchIndex::open_or_create(dir.path()).unwrap();
    assert_eq!(reopened.reader().count(), corpus().len());
    let found = reopened
        .reader()
        .search(&parse("ausschriebung"), 10, &now())
        .unwrap();
    assert_eq!(found.total, 2);
}

#[test]
fn a_read_only_index_is_absent_until_it_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index");
    assert!(SearchIndex::open_read_only(&path).unwrap().is_none());
    fs::create_dir(&path).unwrap();
    assert!(SearchIndex::open_read_only(&path).unwrap().is_none());
    assert_eq!(
        fs::read_dir(&path).unwrap().count(),
        0,
        "a reader must not create files"
    );

    let index = SearchIndex::open_or_create(&path).unwrap();
    let read_only = SearchIndex::open_read_only(&path).unwrap().unwrap();
    assert_eq!(read_only.reader().count(), 0);
    assert!(matches!(read_only.writer(), Err(Error::Invalid(_))));
    drop(index);
}

#[test]
fn a_reader_sees_a_commit_after_reload() {
    let dir = tempfile::tempdir().unwrap();
    let index = SearchIndex::open_or_create(dir.path()).unwrap();
    let mut writer = index.writer().unwrap();
    writer.upsert(&corpus()[0]).unwrap();
    writer.commit().unwrap();

    let read_only = SearchIndex::open_read_only(dir.path()).unwrap().unwrap();
    let reader = read_only.reader();
    assert_eq!(reader.count(), 1);

    for note in &corpus()[1..] {
        writer.upsert(note).unwrap();
    }
    writer.commit().unwrap();
    reader.reload().unwrap();
    assert_eq!(reader.count(), corpus().len());
    // Clones share the reloaded state.
    assert_eq!(read_only.reader().count(), corpus().len());
    assert_eq!(
        reader
            .clone()
            .search(&parse("espresso"), 10, &now())
            .unwrap()
            .total,
        1
    );
}

#[test]
fn an_index_of_another_version_is_wiped_by_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let version_file = dir.path().join("sheaf-index-version");
    {
        let index = SearchIndex::open_or_create(dir.path()).unwrap();
        let mut writer = index.writer().unwrap();
        writer.upsert(&corpus()[0]).unwrap();
        writer.finish().unwrap();
    }
    let version = fs::read_to_string(&version_file).unwrap();

    // Same version: the notes are still there.
    assert_eq!(
        SearchIndex::open_or_create(dir.path())
            .unwrap()
            .reader()
            .count(),
        1
    );

    fs::write(&version_file, "0").unwrap();
    fs::write(dir.path().join("stale-file"), "left over").unwrap();
    assert!(SearchIndex::open_read_only(dir.path()).unwrap().is_none());
    assert!(
        dir.path().join("stale-file").exists(),
        "a reader must not wipe"
    );

    let index = SearchIndex::open_or_create(dir.path()).unwrap();
    assert_eq!(index.reader().count(), 0);
    assert!(index.reader().manifest().unwrap().is_empty());
    assert_eq!(fs::read_to_string(&version_file).unwrap(), version);
    assert!(!dir.path().join("stale-file").exists());
    assert!(SearchIndex::open_read_only(dir.path()).unwrap().is_some());
}

#[test]
fn an_index_that_does_not_open_is_wiped_by_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    {
        let index = SearchIndex::open_or_create(dir.path()).unwrap();
        let mut writer = index.writer().unwrap();
        writer.upsert(&corpus()[0]).unwrap();
        writer.finish().unwrap();
    }
    fs::write(dir.path().join("meta.json"), "{ not json").unwrap();
    assert!(SearchIndex::open_read_only(dir.path()).unwrap().is_none());
    let index = SearchIndex::open_or_create(dir.path()).unwrap();
    assert_eq!(index.reader().count(), 0);

    // A missing version file means the index was never finished.
    drop(index);
    fs::remove_file(dir.path().join("sheaf-index-version")).unwrap();
    assert!(SearchIndex::open_read_only(dir.path()).unwrap().is_none());
    assert_eq!(
        SearchIndex::open_or_create(dir.path())
            .unwrap()
            .reader()
            .count(),
        0
    );
}

#[test]
fn the_manifest_lists_every_note() {
    let notes = corpus();
    let index = index_of(&notes);
    let mut manifest = index.reader().manifest().unwrap();
    manifest.sort();
    let mut expected: Vec<_> = notes
        .iter()
        .map(|note| {
            (
                note.meta.rel_path.clone(),
                note.meta.id.clone(),
                note.meta.modified,
            )
        })
        .collect();
    expected.sort();
    assert_eq!(manifest, expected);

    // After changes spread over several commits, and therefore segments.
    let mut writer = index.writer().unwrap();
    writer.remove_path(&notes[0].meta.rel_path);
    writer.commit().unwrap();
    let changed = notes[1].clone().modified("2026-11-16T11:11:11Z");
    writer.upsert(&changed).unwrap();
    writer.commit().unwrap();
    let mut manifest = index.reader().manifest().unwrap();
    manifest.sort();
    expected.retain(|(path, ..)| *path != notes[0].meta.rel_path);
    for entry in &mut expected {
        if entry.0 == changed.meta.rel_path {
            entry.2 = changed.meta.modified;
        }
    }
    assert_eq!(manifest, expected);
}

#[test]
fn tags_are_counted_with_their_parents() {
    let mut notes = corpus();
    notes.push(note("x.md", "X\n\n#Arbeit/Projekte/Sheaf #ARBEIT #büro"));
    notes.push(note("y.md", "Y\n\n#Buro #arbeit"));
    let reader = index_of(&notes).reader();
    let expected = [
        ("arbeit", 4),
        ("arbeit/projekte", 2),
        ("Arbeit/Projekte/Sheaf", 1),
        ("büro", 2),
        ("einkauf", 1),
        ("projekt", 1),
        ("projekt/sheaf", 1),
        ("rezepte", 1),
        ("rezepte/backen", 1),
    ];
    assert_eq!(
        reader.tags().unwrap(),
        expected.map(|(tag, count)| (tag.to_owned(), count))
    );
    // The counts are what the filter finds.
    for (tag, count) in expected {
        let found = reader
            .search(&parse(&format!("#{tag}")), 50, &now())
            .unwrap();
        assert_eq!(found.total, count, "#{tag}");
    }
}

#[test]
fn folders_are_counted_without_their_subfolders() {
    let reader = index_of(&corpus()).reader();
    let expected = [
        ("", 1),
        ("Arbeit", 1),
        ("Arbeit/Projekte", 1),
        ("Firma/Arbeit", 1),
        ("Meine Notizen/Reisen", 1),
        ("Privat", 1),
        ("Privat/Rezepte", 1),
    ];
    assert_eq!(
        reader.folders().unwrap(),
        expected.map(|(folder, count)| (folder.to_owned(), count))
    );
}

#[test]
fn titles_complete_from_a_prefix() {
    let mut notes = corpus();
    notes.push(note("a.md", "Reisekosten 2026"));
    notes.push(note("b.md", "Ärzte"));
    notes.push(note("c.md", "Aerztehaus"));
    let reader = index_of(&notes).reader();
    let complete = |prefix: &str, limit| {
        let notes = reader.titles_with_prefix(prefix, limit).unwrap();
        notes.into_iter().map(|note| note.title).collect::<Vec<_>>()
    };
    assert_eq!(
        complete("reise", 10),
        ["Reise nach Zürich", "Reisekosten 2026"]
    );
    assert_eq!(complete("Reise nach Zür", 10), ["Reise nach Zürich"]);
    assert_eq!(complete("reise nach zuer", 10), ["Reise nach Zürich"]);
    assert_eq!(complete("REISE", 1), ["Reise nach Zürich"]);
    // Umlauts sort as in a German phone book: ä next to ae.
    assert_eq!(
        complete("a", 10),
        [
            "Ärzte",
            "Aerztehaus",
            "Apfelkuchen",
            "Ausschreibung Serverraum"
        ]
    );
    assert_eq!(complete("är", 10), ["Ärzte", "Aerztehaus"]);
    assert_eq!(complete("aer", 10), ["Ärzte", "Aerztehaus"]);
    assert_eq!(complete("arz", 10), ["Ärzte"]);
    assert_eq!(complete("nach", 10), [""; 0]);
    assert_eq!(complete("", 3).len(), 3);
    assert_eq!(complete("", 100).len(), notes.len());
}

#[test]
fn backlinks_find_notes_linking_to_a_title() {
    let mut notes = corpus();
    notes.push(
        note("a.md", "Verweise\n\nSiehe [[ausschreibung serverraum|die Ausschreibung]] und [[Einkäufe#Liste]].")
            .modified("2026-11-16T10:00:00Z"),
    );
    let reader = index_of(&notes).reader();
    let backlinks = |title: &str| {
        let notes = reader.backlinks(title).unwrap();
        notes.into_iter().map(|note| note.title).collect::<Vec<_>>()
    };
    assert_eq!(
        backlinks("Ausschreibung Serverraum"),
        ["Verweise", "Sheaf Roadmap"]
    );
    assert_eq!(
        backlinks("AUSSCHREIBUNG SERVERRAUM"),
        ["Verweise", "Sheaf Roadmap"]
    );
    assert_eq!(backlinks("Einkäufe"), ["Verweise"]);
    // A link names one title; it is not a search.
    assert_eq!(backlinks("Einkaufe"), [""; 0]);
    assert_eq!(backlinks("Ausschreibung"), [""; 0]);
}

#[test]
fn handles_can_be_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SearchIndex>();
    assert_send_sync::<sheaf_core::Reader>();
    fn assert_send<T: Send>() {}
    assert_send::<sheaf_core::Writer>();
}

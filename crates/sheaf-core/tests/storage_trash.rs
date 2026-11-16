//! The trash: delete, list, restore, purge and expiry.

// Helpers outside `#[test]` functions are not covered by `allow-unwrap-in-tests`.
#![allow(clippy::unwrap_used)]

use std::fs;

use jiff::{SignedDuration, Timestamp};
use sheaf_core::{CreateOptions, Error, NoteId, NoteMeta, NoteStore, TRASH_MAX_AGE};
use tempfile::TempDir;

fn store() -> (TempDir, NoteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = NoteStore::new(dir.path());
    (dir, store)
}

fn create_in(store: &NoteStore, folder: &str, text: &str) -> NoteMeta {
    let options = CreateOptions {
        folder: folder.to_owned(),
        ..CreateOptions::default()
    };
    store.create(text, &options).unwrap()
}

fn read(dir: &TempDir, rel_path: &str) -> String {
    fs::read_to_string(dir.path().join(rel_path)).unwrap()
}

fn time(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[test]
fn trash_and_restore_bring_the_file_back_unchanged() {
    let (dir, store) = store();
    let meta = create_in(&store, "Arbeit/Q4", "Bericht\n- [ ] prüfen\n");
    let (_, link) = store
        .add_attachment(&meta.id, "zahlen.csv", b"1;2")
        .unwrap();
    let original = read(&dir, "Arbeit/Q4/Bericht.md");
    let deleted = time("2026-11-16T12:00:00Z");

    let trashed = store.trash(&meta.id, deleted).unwrap();
    assert_eq!(trashed.rel_path, ".trash/Bericht.md");
    assert_eq!(trashed.id, meta.id);
    assert_eq!(trashed.title, "Bericht");
    assert!(store.walk().unwrap().is_empty());
    assert!(matches!(store.get(&meta.id), Err(Error::NotFound(_))));
    assert!(matches!(store.resolve("Bericht"), Err(Error::NotFound(_))));
    assert!(dir.path().join(".trash").join(&link).is_file());
    assert!(!dir.path().join("Arbeit/Q4/_attachments").exists());

    let text = read(&dir, ".trash/Bericht.md");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[3].starts_with("deleted: 2026-11-16T"), "{}", lines[3]);
    assert_eq!(
        lines[4..],
        ["from: Arbeit/Q4", "---", "Bericht", "- [ ] prüfen"]
    );

    let listed = store.list_trash().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].meta, trashed);
    assert_eq!(listed[0].deleted, deleted);
    assert_eq!(listed[0].from, "Arbeit/Q4");

    // The folder is gone in the meantime and comes back with the note.
    fs::remove_dir_all(dir.path().join("Arbeit")).unwrap();
    let restored = store.restore(&meta.id).unwrap();
    assert_eq!(restored.rel_path, "Arbeit/Q4/Bericht.md");
    assert_eq!(restored.id, meta.id);
    assert_eq!(restored.created, meta.created);
    assert_eq!(read(&dir, "Arbeit/Q4/Bericht.md"), original);
    assert!(dir.path().join("Arbeit/Q4").join(&link).is_file());
    assert!(store.list_trash().unwrap().is_empty());
    assert!(!dir.path().join(".trash/Bericht.md").exists());
    assert_eq!(store.get(&meta.id).unwrap().meta, restored);
    assert!(matches!(store.restore(&meta.id), Err(Error::NotFound(_))));
}

#[test]
fn names_collide_in_the_trash_and_on_restore() {
    let (dir, store) = store();
    let first = create_in(&store, "", "Notiz\neins\n");
    let second = create_in(&store, "Ordner", "Notiz\nzwei\n");
    let now = time("2026-11-16T12:00:00Z");

    assert_eq!(
        store.trash(&first.id, now).unwrap().rel_path,
        ".trash/Notiz.md"
    );
    assert_eq!(
        store.trash(&second.id, now).unwrap().rel_path,
        ".trash/Notiz 2.md"
    );
    assert!(read(&dir, ".trash/Notiz.md").contains("\nfrom: \"\"\n"));

    let taken = create_in(&store, "", "Notiz\ndrei\n");
    assert_eq!(taken.rel_path, "Notiz.md");
    let restored = store.restore(&first.id).unwrap();
    assert_eq!(restored.rel_path, "Notiz 2.md");
    assert_eq!(restored.folder, "");
    assert!(read(&dir, "Notiz 2.md").ends_with("---\nNotiz\neins\n"));
    assert!(read(&dir, "Notiz.md").ends_with("---\nNotiz\ndrei\n"));
    assert_eq!(
        store.restore(&second.id).unwrap().rel_path,
        "Ordner/Notiz 2.md"
    );
}

#[test]
fn a_foreign_note_gets_its_id_when_trashed() {
    let (dir, store) = store();
    fs::write(dir.path().join("Fremd.md"), "Fremd\r\nText\r\n").unwrap();

    let trashed = store
        .trash(
            &NoteId::provisional("Fremd.md"),
            time("2026-11-16T12:00:00Z"),
        )
        .unwrap();
    assert!(!trashed.id.is_provisional());
    assert!(!dir.path().join("Fremd.md").exists());
    assert_eq!(read(&dir, ".trash/Fremd.md").matches("\r\n").count(), 8);

    let restored = store.restore(&trashed.id).unwrap();
    assert_eq!(restored.id, trashed.id);
    assert_eq!(restored.rel_path, "Fremd.md");
    let text = read(&dir, "Fremd.md");
    assert!(text.starts_with(&format!("---\r\nid: {}\r\ncreated: ", trashed.id)));
    assert!(text.ends_with("\r\n---\r\nFremd\r\nText\r\n"));
    assert!(!text.contains("deleted") && !text.contains("from"));
}

#[test]
fn purge_removes_the_note_and_its_attachments() {
    let (dir, store) = store();
    let keep = create_in(&store, "", "Bleibt\n");
    let gone = create_in(&store, "", "Weg\n");
    store.add_attachment(&keep.id, "a.txt", b"a").unwrap();
    store.add_attachment(&gone.id, "b.txt", b"b").unwrap();
    let now = time("2026-11-16T12:00:00Z");
    store.trash(&keep.id, now).unwrap();
    store.trash(&gone.id, now).unwrap();

    assert!(matches!(
        store.purge(&NoteId::new()),
        Err(Error::NotFound(_))
    ));
    store.purge(&gone.id).unwrap();
    let left: Vec<NoteId> = store
        .list_trash()
        .unwrap()
        .into_iter()
        .map(|note| note.meta.id)
        .collect();
    assert_eq!(left, std::slice::from_ref(&keep.id));
    assert!(!dir.path().join(".trash/Weg.md").exists());
    assert!(
        !dir.path()
            .join(".trash/_attachments")
            .join(gone.id.as_str())
            .exists()
    );
    assert!(
        dir.path()
            .join(".trash/_attachments")
            .join(keep.id.as_str())
            .join("a.txt")
            .is_file()
    );
}

#[test]
fn notes_expire_after_exactly_thirty_days() {
    let (dir, store) = store();
    assert!(store.list_trash().unwrap().is_empty());
    assert!(
        store
            .purge_expired(Timestamp::now(), TRASH_MAX_AGE)
            .unwrap()
            .is_empty()
    );

    let old = create_in(&store, "", "Alt\n");
    let newer = create_in(&store, "", "Neuer\n");
    let newest = create_in(&store, "", "Neu\n");
    store.add_attachment(&old.id, "a.txt", b"a").unwrap();
    let deleted = time("2026-11-01T08:00:00Z");
    store.trash(&old.id, deleted).unwrap();
    store
        .trash(&newest.id, time("2026-11-03T08:00:00Z"))
        .unwrap();
    store
        .trash(&newer.id, time("2026-11-02T08:00:00Z"))
        .unwrap();

    let order: Vec<String> = store
        .list_trash()
        .unwrap()
        .into_iter()
        .map(|note| note.meta.title)
        .collect();
    assert_eq!(order, ["Neu", "Neuer", "Alt"]);

    let expiry = time("2026-12-01T08:00:00Z");
    assert_eq!(expiry.duration_since(deleted), TRASH_MAX_AGE);
    let just_before = expiry - SignedDuration::from_secs(1);
    assert!(
        store
            .purge_expired(just_before, TRASH_MAX_AGE)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.list_trash().unwrap().len(), 3);

    let removed = store.purge_expired(expiry, TRASH_MAX_AGE).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].meta.id, old.id);
    assert_eq!(removed[0].deleted, deleted);
    assert!(!dir.path().join(".trash/Alt.md").exists());
    assert!(
        !dir.path()
            .join(".trash/_attachments")
            .join(old.id.as_str())
            .exists()
    );
    assert_eq!(store.list_trash().unwrap().len(), 2);

    let removed = store.purge_expired(expiry, SignedDuration::ZERO).unwrap();
    assert_eq!(removed.len(), 2);
    assert!(store.list_trash().unwrap().is_empty());
}

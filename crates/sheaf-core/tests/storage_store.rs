//! The notes folder end to end: create, read, change, rename and move notes.

// Helpers outside `#[test]` functions are not covered by `allow-unwrap-in-tests`.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use jiff::Timestamp;
use sheaf_core::{CreateOptions, Error, NoteId, NoteMeta, NoteStore, TaskRef, WriteOptions};
use tempfile::TempDir;

fn store() -> (TempDir, NoteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = NoteStore::new(dir.path());
    (dir, store)
}

fn create(store: &NoteStore, text: &str) -> NoteMeta {
    store.create(text, &CreateOptions::default()).unwrap()
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

fn write(dir: &TempDir, rel_path: &str, text: &str) {
    let path = dir.path().join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn time(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn paths(store: &NoteStore) -> Vec<String> {
    store
        .walk()
        .unwrap()
        .into_iter()
        .map(|entry| entry.rel_path)
        .collect()
}

fn no_temp_files(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(!name.starts_with(".sheaf-tmp-"), "left over: {name}");
    }
}

#[test]
fn create_writes_minimal_frontmatter() {
    let (dir, store) = store();
    let options = CreateOptions {
        created: Some(time("2026-11-16T09:12:00Z")),
        ..CreateOptions::default()
    };
    let meta = store
        .create("Einkauf\n- [ ] Milch\n- [x] Brot #essen", &options)
        .unwrap();

    assert_eq!(meta.rel_path, "Einkauf.md");
    assert_eq!(meta.folder, "");
    assert_eq!(meta.title, "Einkauf");
    assert!(!meta.id.is_provisional());
    assert_eq!(meta.created, time("2026-11-16T09:12:00Z"));
    assert_eq!(meta.tags, ["essen"]);
    assert_eq!((meta.tasks_open, meta.tasks_total), (1, 2));
    assert!(!meta.pinned && !meta.conflict);

    let text = read(&dir, "Einkauf.md");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines[1], format!("id: {}", meta.id));
    assert!(lines[2].starts_with("created: 2026-11-16T"), "{}", lines[2]);
    assert_eq!(
        lines[3..],
        ["---", "Einkauf", "- [ ] Milch", "- [x] Brot #essen"]
    );
    assert!(text.ends_with("#essen\n"));
    no_temp_files(dir.path());
}

#[test]
fn file_names_follow_titles() {
    let (_dir, store) = store();
    let cases = [
        ("Einkauf", "Einkauf.md"),
        ("Einkauf\nanderer Text", "Einkauf 2.md"),
        ("# Einkauf", "Einkauf 3.md"),
        ("Soll/Haben\\Rest", "Soll-Haben-Rest.md"),
        ("Fragen: wer? *was*", "Fragen- wer was.md"),
        (".versteckt", "_versteckt.md"),
        ("", "Untitled.md"),
        ("\n\n", "Untitled 2.md"),
        ("Tab\there", "Tab here.md"),
    ];
    for (text, name) in cases {
        assert_eq!(create(&store, text).rel_path, name, "{text:?}");
    }

    let long = create(&store, &"ü".repeat(200));
    assert_eq!(long.rel_path, format!("{}.md", "ü".repeat(60)));
    assert_eq!(long.title, "ü".repeat(200));
    let long_again = create(&store, &"ü".repeat(200));
    assert_eq!(long_again.rel_path, format!("{} 2.md", "ü".repeat(60)));
}

#[test]
fn create_honours_folder_tags_and_pin() {
    let (dir, store) = store();
    let options = CreateOptions {
        folder: "Arbeit/Q4".to_owned(),
        tags: vec!["büro".to_owned(), "#neu".to_owned(), "milch".to_owned()],
        pinned: true,
        created: None,
    };
    let meta = store.create("Plan\r\nmit #Milch\r\n", &options).unwrap();
    assert_eq!(meta.rel_path, "Arbeit/Q4/Plan.md");
    assert_eq!(meta.folder, "Arbeit/Q4");
    assert!(meta.pinned);
    assert_eq!(meta.tags, ["büro", "neu", "Milch"]);

    let text = read(&dir, "Arbeit/Q4/Plan.md");
    assert!(text.starts_with("---\r\nid: "));
    assert!(
        text.ends_with("\r\npinned: true\r\ntags: [büro, neu]\r\n---\r\nPlan\r\nmit #Milch\r\n")
    );
    assert!(!text.replace("\r\n", "").contains('\n'));

    for folder in [
        "../outside",
        "/abs",
        "a/../../b",
        ".trash",
        "x/_attachments",
    ] {
        let options = CreateOptions {
            folder: folder.to_owned(),
            ..CreateOptions::default()
        };
        assert!(
            matches!(store.create("x", &options), Err(Error::Invalid(_))),
            "{folder}"
        );
    }
    let options = CreateOptions {
        tags: vec!["two words".to_owned()],
        ..CreateOptions::default()
    };
    assert!(matches!(
        store.create("x", &options),
        Err(Error::Invalid(_))
    ));
    assert_eq!(paths(&store), ["Arbeit/Q4/Plan.md"]);
}

#[test]
fn walk_sees_only_notes_inside_the_root() {
    let (dir, store) = store();
    assert!(
        NoteStore::new(dir.path().join("missing"))
            .walk()
            .unwrap()
            .is_empty()
    );

    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.md"), "secret").unwrap();
    for (path, text) in [
        ("b.md", "bb"),
        ("a/deep/er/note.md", "note"),
        ("a/.hidden.md", "x"),
        (".trash/gone.md", "x"),
        (".sheaf/templates/t.md", "x"),
        (".git/HEAD.md", "x"),
        ("a/_attachments/01/readme.md", "x"),
        ("a/picture.png", "x"),
        ("a/notes.txt", "x"),
        ("empty/.keep", ""),
    ] {
        write(&dir, path, text);
    }
    std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.md"), dir.path().join("link.md"))
        .unwrap();

    let entries = store.walk().unwrap();
    let seen: Vec<(&str, u64)> = entries
        .iter()
        .map(|entry| (entry.rel_path.as_str(), entry.size))
        .collect();
    assert_eq!(seen, [("a/deep/er/note.md", 4), ("b.md", 2)]);
    assert!(entries[0].modified > time("2026-01-01T00:00:00Z"));
    assert_eq!(
        store.folders().unwrap(),
        ["a", "a/deep", "a/deep/er", "empty"]
    );
    assert_eq!(store.all_meta().unwrap().len(), 2);
}

#[test]
fn unreadable_files_are_skipped_not_fatal() {
    let (dir, store) = store();
    create(
        &store,
        "# Gut\n\n## Teil\n\nSiehe [[Andere]] und [[Dritte|so]].\n",
    );
    fs::write(dir.path().join("kaputt.md"), [0xff, 0xfe, 0x00, 0x41]).unwrap();

    let docs = store.all_docs().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].meta.title, "Gut");
    assert_eq!(docs[0].headings, ["Teil"]);
    assert_eq!(docs[0].links, ["Andere", "Dritte"]);
    assert_eq!(
        docs[0].body,
        "# Gut\n\n## Teil\n\nSiehe [[Andere]] und [[Dritte|so]].\n"
    );
    assert!(matches!(store.load("kaputt.md"), Err(Error::Io(_))));
    assert!(matches!(store.load("fehlt.md"), Err(Error::NotFound(_))));
    assert!(matches!(
        store.load("../etc/passwd"),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn files_sheaf_did_not_change_stay_byte_identical() {
    const ID: &str = "01JD3K8Z6Q4X7V9B2M5N8P0R3T";
    let cases = [
        "Nur Text\n".to_owned(),
        "Ohne Zeilenende".to_owned(),
        format!("---\nid: {ID}\n---\nTitel\n"),
        format!(
            "---\r\nid: {ID}\r\ncreated: 2026-11-16T10:12:00+01:00\r\n---\r\nTitel\r\nText\r\n"
        ),
        format!("\u{feff}---\nid: {ID}\n---\nTitel\n"),
        format!(
            "---\n# Kommentar\nid: {ID}   # bleibt\naliases: ['a', \"b\"]\nnested:\n  key: [1, 2]\n---\nTitel\n"
        ),
        format!("---\ntags:\n  - eins\n  - zwei\nid: {ID}\n---\n\nTitel #drei\n"),
        "---\nkaputt, kein YAML\n---\nText\n".to_owned(),
        "---\nid: offen\nnie geschlossen\n".to_owned(),
    ];
    for original in cases {
        let (dir, store) = store();
        write(&dir, "note.md", &original);
        let note = store.load("note.md").unwrap();
        let id = note.meta.id.clone();
        let stamped = !id.is_provisional();

        // Reading, resolving and writing back what is there change nothing.
        store.all_docs().unwrap();
        let same = store
            .write_body(&id, note.body(), &WriteOptions::default())
            .unwrap();
        assert_eq!(same.id, id);
        assert_eq!(same.modified, note.meta.modified);
        store.set_pinned(&id, false).unwrap();
        store.append(&id, "").unwrap();
        assert_eq!(read(&dir, "note.md"), original, "{original:?}");

        // A real change touches only its own line, and undoing it restores the file.
        if stamped {
            let pinned = store.set_pinned(&id, true).unwrap();
            assert!(pinned.pinned);
            assert_ne!(read(&dir, "note.md"), original);
            assert_eq!(
                read(&dir, "note.md")
                    .replace("pinned: true\r\n", "")
                    .replace("pinned: true\n", ""),
                original
            );
            let unpinned = store.set_pinned(&id, false).unwrap();
            assert!(!unpinned.pinned);
            assert_eq!(read(&dir, "note.md"), original, "{original:?}");
        }
    }
}

#[test]
fn known_keys_are_read_from_foreign_frontmatter() {
    let (dir, store) = store();
    write(
        &dir,
        "note.md",
        "\u{feff}---\r\ntags:\r\n  - eins\r\n  - Zwei\r\ncreated: 2020-02-02T02:02:02+02:00\r\npinned: true\r\n---\r\n# Titel\r\n\r\n#zwei #drei\r\n",
    );
    let meta = store.load("note.md").unwrap().meta;
    assert!(meta.id.is_provisional());
    assert_eq!(meta.title, "Titel");
    assert_eq!(meta.tags, ["eins", "Zwei", "drei"]);
    assert_eq!(meta.created, time("2020-02-02T00:02:02Z"));
    assert!(meta.pinned);
}

#[test]
fn broken_frontmatter_is_kept_as_text() {
    let (dir, store) = store();
    let original = "---\nkaputt, kein YAML\n---\nText\n";
    write(&dir, "note.md", original);
    let note = store.load("note.md").unwrap();
    assert_eq!(note.body(), original);
    assert_eq!(note.meta.title, "---");

    let meta = store.append(&note.meta.id, "mehr").unwrap();
    let text = read(&dir, "note.md");
    assert!(text.starts_with(&format!("---\nid: {}\ncreated: ", meta.id)));
    assert!(text.ends_with(&format!("\n---\n{original}mehr\n")));
    assert_eq!(
        store.get(&meta.id).unwrap().body(),
        format!("{original}mehr\n")
    );
}

#[test]
fn first_write_replaces_the_provisional_id() {
    let (dir, store) = store();
    write(&dir, "Fremd.md", "Fremd\nText\n");

    let before = store.all_meta().unwrap().remove(0);
    assert_eq!(before.id, NoteId::provisional("Fremd.md"));
    assert_eq!(store.resolve(before.id.as_str()).unwrap().id, before.id);
    assert_eq!(read(&dir, "Fremd.md"), "Fremd\nText\n");

    let after = store.append(&before.id, "neu").unwrap();
    assert!(!after.id.is_provisional());
    assert_eq!(after.rel_path, "Fremd.md");
    let drift = after.created.duration_since(before.created).abs();
    assert!(drift < jiff::SignedDuration::from_secs(1), "{drift:?}");

    let text = read(&dir, "Fremd.md");
    assert!(text.starts_with(&format!("---\nid: {}\ncreated: ", after.id)));
    assert!(text.ends_with("\n---\nFremd\nText\nneu\n"));
    assert!(matches!(store.get(&before.id), Err(Error::NotFound(_))));
    assert_eq!(store.get(&after.id).unwrap().meta, after);

    // The id is stable from here on.
    let again = store.append(&after.id, "mehr").unwrap();
    assert_eq!(again.id, after.id);
    assert_eq!(again.created, after.created);
    assert_eq!(
        NoteStore::new(dir.path()).get(&after.id).unwrap().meta,
        again
    );
}

#[test]
fn write_body_detects_changes_by_others() {
    let (dir, store) = store();
    let meta = create(&store, "Titel\nText\n");
    let seen = store.get(&meta.id).unwrap().fingerprint;
    assert_eq!(store.fingerprint(&meta.id).unwrap(), seen);

    let guarded = WriteOptions {
        rename: false,
        expected: Some(seen),
    };
    store
        .write_body(&meta.id, "Titel\nmeine Fassung\n", &guarded)
        .unwrap();
    let mine = read(&dir, "Titel.md");

    // The same fingerprint is stale now, whoever changed the file.
    let stale = store.write_body(&meta.id, "Titel\nzu spät\n", &guarded);
    assert!(matches!(stale, Err(Error::Conflict(path)) if path == "Titel.md"));
    assert_eq!(read(&dir, "Titel.md"), mine);

    fs::write(
        dir.path().join("Titel.md"),
        format!("{mine}extern angehängt\n"),
    )
    .unwrap();
    let guarded = WriteOptions {
        rename: true,
        expected: Some(seen),
    };
    assert!(matches!(
        store.write_body(&meta.id, "Neu\n", &guarded),
        Err(Error::Conflict(_))
    ));
    assert!(read(&dir, "Titel.md").ends_with("extern angehängt\n"));

    let current = WriteOptions {
        rename: false,
        expected: Some(store.fingerprint(&meta.id).unwrap()),
    };
    let written = store
        .write_body(&meta.id, "Titel\nneu\n", &current)
        .unwrap();
    assert_eq!(written.id, meta.id);
    assert!(read(&dir, "Titel.md").ends_with("---\nTitel\nneu\n"));
    no_temp_files(dir.path());
}

#[test]
fn write_body_keeps_frontmatter_and_can_rename() {
    let (dir, store) = store();
    let meta = create(&store, "Alt\nText\n");
    let head = read(&dir, "Alt.md")
        .strip_suffix("Alt\nText\n")
        .unwrap()
        .to_owned();
    let rename = WriteOptions {
        rename: true,
        expected: None,
    };

    let kept = store
        .write_body(&meta.id, "Neu\nText\n", &WriteOptions::default())
        .unwrap();
    assert_eq!(
        (kept.rel_path.as_str(), kept.title.as_str()),
        ("Alt.md", "Neu")
    );
    assert_eq!(read(&dir, "Alt.md"), format!("{head}Neu\nText\n"));

    // Without a title change the name stays, even though it does not match.
    let kept = store
        .write_body(&meta.id, "Neu\nmehr Text\n", &rename)
        .unwrap();
    assert_eq!(kept.rel_path, "Alt.md");

    let renamed = store
        .write_body(&meta.id, "Ganz neu\nText\n", &rename)
        .unwrap();
    assert_eq!(renamed.rel_path, "Ganz neu.md");
    assert_eq!(renamed.id, meta.id);
    assert_eq!(paths(&store), ["Ganz neu.md"]);
    assert_eq!(read(&dir, "Ganz neu.md"), format!("{head}Ganz neu\nText\n"));
    assert_eq!(store.get(&meta.id).unwrap().meta, renamed);

    // A taken name gets a number, and a numbered file is left alone.
    let other = create(&store, "Besetzt\n");
    let numbered = store
        .write_body(&meta.id, "Besetzt\nText\n", &rename)
        .unwrap();
    assert_eq!(numbered.rel_path, "Besetzt 2.md");
    assert_eq!(
        store.sync_filename(&meta.id).unwrap().rel_path,
        "Besetzt 2.md"
    );
    assert_eq!(store.get(&other.id).unwrap().meta.rel_path, "Besetzt.md");

    let emptied = store.write_body(&meta.id, "", &rename).unwrap();
    assert_eq!(
        (emptied.rel_path.as_str(), emptied.title.as_str()),
        ("Untitled.md", "")
    );
    assert_eq!(read(&dir, "Untitled.md"), head);
}

#[test]
fn sync_filename_follows_the_title() {
    let (dir, store) = store();
    write(&dir, "Ordner/alter name.md", "# Der Titel\n\nText\n");
    let id = store.load("Ordner/alter name.md").unwrap().meta.id;

    let meta = store.sync_filename(&id).unwrap();
    assert_eq!(meta.rel_path, "Ordner/Der Titel.md");
    assert!(!meta.id.is_provisional());
    assert_eq!(paths(&store), ["Ordner/Der Titel.md"]);
    assert_eq!(store.sync_filename(&meta.id).unwrap(), meta);
}

#[test]
fn append_and_prepend_keep_the_rest_of_the_file() {
    let (dir, store) = store();
    let cases = [
        // (body, appended, prepended)
        ("", "neu\n", "neu\n"),
        ("Titel", "Titel\nneu\n", "Titel\nneu\n"),
        ("Titel\n", "Titel\nneu\n", "Titel\nneu\n"),
        (
            "# Titel\n\nText",
            "# Titel\n\nText\nneu\n",
            "# Titel\n\nneu\n\nText",
        ),
        (
            "Titel\r\n- a\r\n",
            "Titel\r\n- a\r\nneu\r\n",
            "Titel\r\nneu\r\n- a\r\n",
        ),
    ];
    for (body, appended, prepended) in cases {
        let eol = if body.contains("\r\n") { "\r\n" } else { "\n" };
        let head = format!("---{eol}id: 01JD3K8Z6Q4X7V9B2M5N8P0R3T{eol}---{eol}");
        let id: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();

        write(&dir, "note.md", &format!("{head}{body}"));
        store.append(&id, "neu").unwrap();
        assert_eq!(
            read(&dir, "note.md"),
            format!("{head}{appended}"),
            "append to {body:?}"
        );

        write(&dir, "note.md", &format!("{head}{body}"));
        store.prepend(&id, "neu").unwrap();
        assert_eq!(
            read(&dir, "note.md"),
            format!("{head}{prepended}"),
            "prepend to {body:?}"
        );
    }

    // A file whose frontmatter ends without a line break still gets one before the body.
    write(&dir, "note.md", "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R3T\n---");
    let id: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();
    assert_eq!(store.append(&id, "Titel").unwrap().title, "Titel");
    assert_eq!(
        read(&dir, "note.md"),
        "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R3T\n---\nTitel\n"
    );
}

#[test]
fn tasks_are_toggled_in_place() {
    let (dir, store) = store();
    let body = "\
Liste\n\
- [ ] erster Punkt\n\
- [x] zweiter\n\
  - [ ] verschachtelt\n\
> Zitat\n\
> - [X] im Zitat\n\
\n\
```\n\
- [ ] im Code\n\
```\n\
\n\
- [ ] letzter Punkt\n";
    let meta = create(&store, body);
    let head = read(&dir, "Liste.md")
        .strip_suffix(body)
        .unwrap()
        .to_owned();
    assert_eq!((meta.tasks_open, meta.tasks_total), (3, 5));
    let check = |task: TaskRef, checked: bool| store.set_task(&meta.id, &task, checked);

    let meta_1 = check(TaskRef::Number(1), true).unwrap();
    assert_eq!((meta_1.tasks_open, meta_1.tasks_total), (2, 5));
    check(TaskRef::Text("VERSCHACHTELT".into()), true).unwrap();
    check(TaskRef::Text("im Zitat".into()), false).unwrap();
    check(TaskRef::Number(2), false).unwrap();
    check(TaskRef::Number(5), true).unwrap();
    // Already in the wanted state: nothing is written.
    let before = store.get(&meta.id).unwrap().meta.modified;
    assert_eq!(check(TaskRef::Number(5), true).unwrap().modified, before);

    let expected = body
        .replace("- [ ] erster", "- [x] erster")
        .replace("- [x] zweiter", "- [ ] zweiter")
        .replace("- [ ] verschachtelt", "- [x] verschachtelt")
        .replace("- [X] im Zitat", "- [ ] im Zitat")
        .replace("- [ ] letzter", "- [x] letzter");
    assert_eq!(read(&dir, "Liste.md"), format!("{head}{expected}"));
    assert!(expected.contains("- [ ] im Code"));

    assert!(matches!(
        check(TaskRef::Text("Punkt".into()), true),
        Err(Error::Ambiguous { .. })
    ));
    assert!(matches!(
        check(TaskRef::Text("im Code".into()), true),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        check(TaskRef::Number(6), true),
        Err(Error::NotFound(_))
    ));
    assert_eq!(read(&dir, "Liste.md"), format!("{head}{expected}"));
}

#[test]
fn resolve_prefers_id_then_exact_title_then_prefix() {
    let (_dir, store) = store();
    let einkauf = create(&store, "Einkauf\n");
    let liste = create(&store, "# Einkaufsliste\n");
    let alpha = create_in(&store, "Arbeit", "Projekt Alpha\n");
    let beta = create_in(&store, "Privat", "Projekt Beta\n");
    // A note whose title is the id of another note.
    let decoy = create(&store, &format!("{}\n", einkauf.id));

    assert_eq!(store.resolve(einkauf.id.as_str()).unwrap().id, einkauf.id);
    assert_eq!(
        store
            .resolve(&einkauf.id.as_str().to_lowercase())
            .unwrap()
            .id,
        einkauf.id
    );
    assert_eq!(store.resolve("einkauf").unwrap().id, einkauf.id);
    assert_eq!(store.resolve("  EINKAUFSLISTE ").unwrap().id, liste.id);
    assert_eq!(store.resolve("Einkaufs").unwrap().id, liste.id);
    assert_eq!(store.resolve("projekt b").unwrap().id, beta.id);
    assert_eq!(store.resolve(&decoy.id.to_string()).unwrap().id, decoy.id);

    match store.resolve("Projekt") {
        Err(Error::Ambiguous { query, candidates }) => {
            assert_eq!(query, "Projekt");
            let mut found: Vec<(&str, &str)> = candidates
                .iter()
                .map(|note| (note.title.as_str(), note.folder.as_str()))
                .collect();
            found.sort();
            assert_eq!(
                found,
                [("Projekt Alpha", "Arbeit"), ("Projekt Beta", "Privat")]
            );
            assert!(candidates.iter().any(|note| note.id == alpha.id));
        }
        other => panic!("expected an ambiguous reference, got {other:?}"),
    }

    let twin = create_in(&store, "Privat", "einkauf\n");
    match store.resolve("Einkauf") {
        Err(Error::Ambiguous { candidates, .. }) => {
            let mut ids: Vec<&NoteId> = candidates.iter().map(|note| &note.id).collect();
            ids.sort();
            let mut expected = vec![&einkauf.id, &twin.id];
            expected.sort();
            assert_eq!(ids, expected);
        }
        other => panic!("expected an ambiguous reference, got {other:?}"),
    }

    for missing in ["Urlaub", "", "01JD3K8Z6Q4X7V9B2M5N8P0R3T"] {
        assert!(
            matches!(store.resolve(missing), Err(Error::NotFound(_))),
            "{missing:?}"
        );
    }
}

#[test]
fn attachments_move_with_the_note() {
    let (dir, store) = store();
    let meta = create(&store, "Mit Anhang\n");
    let (same, link) = store
        .add_attachment(&meta.id, "/tmp/Mein Foto (1).PNG", b"png")
        .unwrap();
    assert_eq!(same.id, meta.id);
    assert_eq!(link, format!("_attachments/{}/Mein-Foto--1-.PNG", meta.id));
    let (_, second) = store
        .add_attachment(&meta.id, "Mein Foto (1).PNG", b"other")
        .unwrap();
    assert_eq!(
        second,
        format!("_attachments/{}/Mein-Foto--1--2.PNG", meta.id)
    );
    assert_eq!(fs::read(dir.path().join(&link)).unwrap(), b"png");
    assert_eq!(
        store.attachments_dir(&meta.id).unwrap(),
        dir.path().join("_attachments").join(meta.id.as_str())
    );

    store.append(&meta.id, &format!("![Foto]({link})")).unwrap();
    create_in(&store, "Archiv/2026", "Mit Anhang\n");
    let moved = store.move_to(&meta.id, "Archiv/2026/").unwrap();
    assert_eq!(moved.rel_path, "Archiv/2026/Mit Anhang 2.md");
    assert_eq!(moved.folder, "Archiv/2026");
    assert_eq!(moved.id, meta.id);
    assert!(moved.has_image);
    assert_eq!(
        fs::read(dir.path().join("Archiv/2026").join(&link)).unwrap(),
        b"png"
    );
    assert!(!dir.path().join("_attachments").exists());
    assert_eq!(
        paths(&store),
        ["Archiv/2026/Mit Anhang 2.md", "Archiv/2026/Mit Anhang.md"]
    );
    assert_eq!(store.get(&meta.id).unwrap().meta, moved);

    // Moving to where it already is, and back to the root.
    assert_eq!(store.move_to(&meta.id, "Archiv/2026").unwrap(), moved);
    let back = store.move_to(&meta.id, "").unwrap();
    assert_eq!(back.rel_path, "Mit Anhang 2.md");
    assert!(dir.path().join(&link).is_file());
    assert!(matches!(
        store.move_to(&meta.id, "../x"),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn attaching_gives_a_foreign_note_a_real_id() {
    let (dir, store) = store();
    write(&dir, "Fremd.md", "Fremd\n");
    let provisional = NoteId::provisional("Fremd.md");

    let (meta, link) = store
        .add_attachment(&provisional, "scan.pdf", b"pdf")
        .unwrap();
    assert!(!meta.id.is_provisional());
    assert_eq!(link, format!("_attachments/{}/scan.pdf", meta.id));
    assert!(dir.path().join(&link).is_file());

    let moved = store.move_to(&meta.id, "Ablage").unwrap();
    assert_eq!(moved.id, meta.id);
    assert!(dir.path().join("Ablage").join(&link).is_file());
}

#[test]
fn duplicate_is_a_new_note_with_its_own_attachments() {
    let (dir, store) = store();
    let options = CreateOptions {
        pinned: true,
        created: Some(time("2020-01-01T00:00:00Z")),
        ..CreateOptions::default()
    };
    let original = store.create("Rezept\n", &options).unwrap();
    let (_, link) = store
        .add_attachment(&original.id, "bild.jpg", b"jpg")
        .unwrap();
    let original = store.append(&original.id, &format!("![]({link})")).unwrap();
    let original_text = read(&dir, "Rezept.md");

    let copy = store.duplicate(&original.id).unwrap();
    assert_eq!(copy.rel_path, "Rezept 2.md");
    assert_eq!(copy.title, "Rezept");
    assert_ne!(copy.id, original.id);
    assert!(!copy.id.is_provisional());
    assert!(!copy.pinned);
    assert!(copy.created > time("2026-01-01T00:00:00Z"));

    let copy_link = format!("_attachments/{}/bild.jpg", copy.id);
    assert!(read(&dir, "Rezept 2.md").ends_with(&format!("Rezept\n![]({copy_link})\n")));
    assert_eq!(fs::read(dir.path().join(&copy_link)).unwrap(), b"jpg");
    assert_eq!(read(&dir, "Rezept.md"), original_text);
    assert_eq!(store.get(&original.id).unwrap().meta, original);
}

#[test]
fn conflict_copy_sits_next_to_the_note() {
    let (dir, store) = store();
    let original = create_in(&store, "Arbeit", "Bericht\nStand auf der Platte\n");
    let original_text = read(&dir, "Arbeit/Bericht.md");

    let copy = store
        .save_conflict_copy(&original.id, "Bericht\nmeine Fassung\n")
        .unwrap();
    assert_eq!(copy.rel_path, "Arbeit/Bericht (conflict).md");
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.created, original.created);
    assert!(!copy.conflict);
    assert!(read(&dir, &copy.rel_path).ends_with("---\nBericht\nmeine Fassung\n"));
    assert_eq!(read(&dir, "Arbeit/Bericht.md"), original_text);

    let second = store
        .save_conflict_copy(&original.id, "Bericht\nnoch eine\n")
        .unwrap();
    assert_eq!(second.rel_path, "Arbeit/Bericht (conflict) 2.md");
}

#[test]
fn sync_conflict_copies_do_not_steal_the_id() {
    let (dir, store) = store();
    let original = create(&store, "Einkauf\nMilch\n");
    let text = read(&dir, "Einkauf.md");
    let copy_path = "Einkauf.sync-conflict-20261116-101200-ABCDEFG.md";
    write(&dir, copy_path, &text.replace("Milch", "Brot"));
    write(
        &dir,
        "Einkauf (conflicted copy 2026-11-16 101200).md",
        &text,
    );

    for store in [store, NoteStore::new(dir.path())] {
        let metas = store.all_meta().unwrap();
        assert_eq!(metas.len(), 3);
        for meta in &metas {
            let is_original = meta.rel_path == "Einkauf.md";
            assert_eq!(meta.conflict, !is_original, "{}", meta.rel_path);
            assert_eq!(meta.id == original.id, is_original, "{}", meta.rel_path);
            assert_eq!(meta.id.is_provisional(), !is_original);
        }
        assert_eq!(store.get(&original.id).unwrap().meta.rel_path, "Einkauf.md");
    }

    // Editing a copy makes it a note of its own; the original keeps its id.
    let store = NoteStore::new(dir.path());
    let copy = store
        .append(&NoteId::provisional(copy_path), "Eier")
        .unwrap();
    assert!(!copy.id.is_provisional());
    assert_ne!(copy.id, original.id);
    assert!(read(&dir, copy_path).ends_with("Einkauf\nBrot\nEier\n"));
    assert_eq!(read(&dir, copy_path).matches("\nid: ").count(), 1);
    assert_eq!(read(&dir, "Einkauf.md"), text);
    assert_eq!(
        NoteStore::new(dir.path())
            .get(&original.id)
            .unwrap()
            .meta
            .rel_path,
        "Einkauf.md"
    );
}

#[test]
fn changes_by_other_programs_are_found() {
    let (dir, store) = store();
    let meta = create(&store, "Notiz\nText\n");
    let other = create(&store, "Andere\n");

    fs::create_dir(dir.path().join("Ordner")).unwrap();
    fs::rename(
        dir.path().join("Notiz.md"),
        dir.path().join("Ordner/Umbenannt.md"),
    )
    .unwrap();
    assert_eq!(
        store.get(&meta.id).unwrap().meta.rel_path,
        "Ordner/Umbenannt.md"
    );
    assert_eq!(
        store.append(&meta.id, "mehr").unwrap().rel_path,
        "Ordner/Umbenannt.md"
    );

    // A different note now sits at the path the store remembered.
    fs::rename(
        dir.path().join("Ordner/Umbenannt.md"),
        dir.path().join("tmp.md"),
    )
    .unwrap();
    fs::rename(
        dir.path().join("Andere.md"),
        dir.path().join("Ordner/Umbenannt.md"),
    )
    .unwrap();
    assert_eq!(store.get(&meta.id).unwrap().meta.rel_path, "tmp.md");
    assert_eq!(
        store.get(&other.id).unwrap().meta.rel_path,
        "Ordner/Umbenannt.md"
    );

    write(
        &dir,
        "Neu von aussen.md",
        "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R3T\n---\nNeu\n",
    );
    let synced: NoteId = "01JD3K8Z6Q4X7V9B2M5N8P0R3T".parse().unwrap();
    assert_eq!(store.load("Neu von aussen.md").unwrap().meta.id, synced);
    assert_eq!(store.get(&synced).unwrap().meta.title, "Neu");

    fs::remove_file(dir.path().join("tmp.md")).unwrap();
    assert!(matches!(store.get(&meta.id), Err(Error::NotFound(_))));
    assert!(matches!(
        store.append(&meta.id, "x"),
        Err(Error::NotFound(_))
    ));
    store.refresh();
    assert_eq!(store.all_meta().unwrap().len(), 2);
}

#[test]
fn folders_can_be_created_renamed_and_deleted() {
    let (dir, store) = store();
    store.create_folder("Arbeit/Projekte").unwrap();
    assert_eq!(store.folders().unwrap(), ["Arbeit", "Arbeit/Projekte"]);
    assert!(matches!(
        store.create_folder("Arbeit"),
        Err(Error::Exists(_))
    ));
    assert!(matches!(store.create_folder(""), Err(Error::Invalid(_))));
    assert!(matches!(
        store.create_folder("../x"),
        Err(Error::Invalid(_))
    ));

    let note = create_in(&store, "Arbeit/Projekte", "Plan\n");
    write(&dir, "Arbeit/fremd.md", "Fremd\n");
    let foreign = store.load("Arbeit/fremd.md").unwrap().meta.id;

    assert!(matches!(
        store.rename_folder("Fehlt", "X"),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        store.rename_folder("Arbeit/Projekte", "Arbeit"),
        Err(Error::Exists(_))
    ));
    assert!(matches!(
        store.rename_folder("Arbeit", "Arbeit/Neu"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        store.rename_folder("", "X"),
        Err(Error::Invalid(_))
    ));

    store.rename_folder("Arbeit", "Büro/2026").unwrap();
    assert_eq!(
        store.folders().unwrap(),
        ["Büro", "Büro/2026", "Büro/2026/Projekte"]
    );
    assert_eq!(
        store.get(&note.id).unwrap().meta.rel_path,
        "Büro/2026/Projekte/Plan.md"
    );
    // A provisional id is derived from the path and changes with it.
    assert!(matches!(store.get(&foreign), Err(Error::NotFound(_))));
    assert!(
        store
            .get(&NoteId::provisional("Büro/2026/fremd.md"))
            .is_ok()
    );

    let (_, link) = store.add_attachment(&note.id, "a.txt", b"a").unwrap();
    let now = time("2026-11-16T12:00:00Z");
    let trashed = store.delete_folder("Büro", now).unwrap();
    assert_eq!(trashed.len(), 2);
    assert!(store.folders().unwrap().is_empty());
    assert!(!dir.path().join("Büro").exists());
    assert!(paths(&store).is_empty());

    let listed = store.list_trash().unwrap();
    let mut origins: Vec<(&str, &str)> = listed
        .iter()
        .map(|trashed| (trashed.meta.title.as_str(), trashed.from.as_str()))
        .collect();
    origins.sort();
    assert_eq!(
        origins,
        [("Fremd", "Büro/2026"), ("Plan", "Büro/2026/Projekte")]
    );
    assert!(
        listed
            .iter()
            .all(|trashed| trashed.deleted == now && !trashed.meta.id.is_provisional())
    );
    assert!(dir.path().join(".trash").join(&link).is_file());

    assert!(matches!(
        store.delete_folder("Büro", now),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        store.delete_folder("", now),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn deleting_a_folder_never_deletes_other_files() {
    let (dir, store) = store();
    create_in(&store, "Mix/Unter", "Notiz\n");
    write(&dir, "Mix/vertrag.pdf", "pdf");

    let trashed = store
        .delete_folder("Mix", time("2026-11-16T12:00:00Z"))
        .unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(store.folders().unwrap(), ["Mix"]);
    assert_eq!(read(&dir, "Mix/vertrag.pdf"), "pdf");
}

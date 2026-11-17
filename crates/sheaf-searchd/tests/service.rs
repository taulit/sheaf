//! `sheaf-searchd` end to end: the real binary on a private bus.

#![allow(clippy::unwrap_used)]

mod harness;

use std::fs;
use std::time::{Duration, SystemTime};

use harness::{Fixture, TIMEOUT, error_name, eventually, next, notes_proxy, provider_proxy, sleep};
use sheaf_core::paths::Env;
use sheaf_core::{NoteId, SearchIndex};
use sheaf_searchd::api::{ACTION_OPEN_NOTE, ACTION_QUICK_NOTE, ACTION_SEARCH, NoteRow};
use zbus::Variant;

fn ids(rows: &[NoteRow]) -> Vec<&str> {
    rows.iter().map(|row| row.0.as_str()).collect()
}

fn provisional(rel_path: &str) -> String {
    NoteId::provisional(rel_path).to_string()
}

#[test]
fn finds_notes_written_before_and_after_the_start() {
    let Some(mut fx) = Fixture::new() else { return };
    fx.write(
        "Fruit/apples.md",
        "Apples\n\nCrisp apples from the orchard.\n",
    );
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        let mut changed = notes.receive_changed().await.unwrap();

        let rows = notes.search("orchard", 0).await.unwrap();
        assert_eq!(ids(&rows), [provisional("Fruit/apples.md")]);
        let (_, title, snippet, folder, modified) = &rows[0];
        assert_eq!(title, "Apples");
        assert!(snippet.contains("orchard"), "{snippet}");
        assert_eq!(folder, "Fruit");
        assert!(*modified > 1_700_000_000);

        fx.write(
            "later.md",
            "Later\n\nWritten while the service runs: quince.\n",
        );
        // Skips the signal of the first build, which may arrive this late.
        let later = provisional("later.md");
        let signalled = eventually(async || {
            let signal = next(&mut changed, TIMEOUT).await?;
            signal.args().unwrap().ids.contains(&later).then_some(())
        })
        .await;
        assert!(signalled.is_some(), "no Changed for later.md");
        let rows = notes.search("quince", 0).await.unwrap();
        assert_eq!(ids(&rows), [provisional("later.md")]);

        fs::remove_file(fx.notes().join("later.md")).unwrap();
        let gone = eventually(async || {
            let rows = notes.search("quince", 0).await.unwrap();
            rows.is_empty().then_some(())
        })
        .await;
        assert!(gone.is_some(), "a removed note is still found");

        let recent = notes.recent(0).await.unwrap();
        assert_eq!(ids(&recent), [provisional("Fruit/apples.md")]);
    });
}

#[test]
fn created_and_appended_notes_are_searchable_at_once() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        let mut changed = notes.receive_changed().await.unwrap();

        let quick = notes.create("Quick\n\nmango", "").await.unwrap();
        let signal = next(&mut changed, TIMEOUT).await.expect("Changed");
        assert_eq!(signal.args().unwrap().ids, std::slice::from_ref(&quick));
        let root = notes.create("Root\n\npapaya", "/").await.unwrap();
        let nested = notes.create("Nested\n\nguava", "Work/Ideas").await.unwrap();

        for (word, id, folder) in [
            ("mango", &quick, "Quick Notes"),
            ("papaya", &root, ""),
            ("guava", &nested, "Work/Ideas"),
        ] {
            let rows = notes.search(word, 0).await.unwrap();
            assert_eq!(ids(&rows), [id.as_str()], "{word}");
            assert_eq!(rows[0].3, folder);
        }
        assert!(fx.notes().join("Quick Notes/Quick.md").is_file());
        assert!(fx.notes().join("Root.md").is_file());
        assert!(fx.notes().join("Work/Ideas/Nested.md").is_file());

        notes.append(&root, "and kiwano").await.unwrap();
        let rows = notes.search("kiwano", 0).await.unwrap();
        assert_eq!(ids(&rows), [root.as_str()]);

        // A note Sheaf did not write gets a real id with the first append.
        fx.write("plain.md", "Plain\n");
        notes.sync().await.unwrap();
        notes
            .append(&provisional("plain.md"), "feijoa")
            .await
            .unwrap();
        let rows = notes.search("feijoa", 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].0.starts_with('p'), "{}", rows[0].0);
    });
}

#[test]
fn failures_have_sheaf_error_names() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;

        for id in ["01ARZ3NDEKTSV4RRFFQ69G5FAV", "no such note"] {
            let err = notes.append(id, "text").await.unwrap_err();
            assert_eq!(error_name(&err), "io.github.taulit.Sheaf1.Error.NotFound");
        }
        let err = notes.create("Escape", "../outside").await.unwrap_err();
        assert_eq!(error_name(&err), "io.github.taulit.Sheaf1.Error.Invalid");

        let err = notes.open("01ARZ3NDEKTSV4RRFFQ69G5FAV").await.unwrap_err();
        assert_eq!(
            error_name(&err),
            "io.github.taulit.Sheaf1.Error.AppUnavailable"
        );
        let err = notes.quick_note().await.unwrap_err();
        assert_eq!(
            error_name(&err),
            "io.github.taulit.Sheaf1.Error.AppUnavailable"
        );

        // The shell ignores errors of these two, so they are only logged.
        let provider = provider_proxy(&conn).await;
        provider.activate_result("x", &["a"], 0).await.unwrap();
        provider.launch_search(&["a"], 0).await.unwrap();
    });
}

#[test]
fn search_provider_returns_ids_and_metas() {
    let Some(mut fx) = Fixture::new() else { return };
    fx.write(
        "lemon.md",
        "Lemon tart\n\nZest of a lemon, butter, sugar.\n",
    );
    fx.write("lime.md", "Lime pie\n\nA lemon or a lime will do.\n");
    fx.write("blank.md", "\n\n");
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let provider = provider_proxy(&conn).await;
        let lemon = provisional("lemon.md");
        let lime = provisional("lime.md");
        let blank = provisional("blank.md");

        let mut found = provider.get_initial_result_set(&["lemon"]).await.unwrap();
        found.sort();
        let mut expected = vec![lemon.clone(), lime.clone()];
        expected.sort();
        assert_eq!(found, expected);
        let narrowed = provider
            .get_subsearch_result_set(&[&lemon, &lime], &["lemon", "tart"])
            .await
            .unwrap();
        assert_eq!(narrowed, std::slice::from_ref(&lemon));

        let metas = provider
            .get_result_metas(&[&lemon, &lime, &blank, "01ARZ3NDEKTSV4RRFFQ69G5FAV", "junk"])
            .await
            .unwrap();
        assert_eq!(metas.len(), 3, "unknown ids are skipped: {metas:?}");
        let text =
            |meta: &std::collections::HashMap<String, zbus::OwnedVariant>, key: &str| match &**meta
                .get(key)
                .unwrap_or_else(|| panic!("no {key}"))
            {
                Variant::Str(text) => text.to_string(),
                other => panic!("{key} is not a string: {other:?}"),
            };
        for meta in &metas {
            assert_eq!(meta.len(), 4, "{meta:?}");
            assert_eq!(text(meta, "gicon"), "io.github.taulit.Sheaf");
        }
        assert_eq!(text(&metas[0], "id"), lemon);
        assert_eq!(text(&metas[0], "name"), "Lemon tart");
        assert!(text(&metas[0], "description").contains("lemon"));
        // The snippet of the last search, which matched "lemon" here, not the beginning.
        assert_eq!(text(&metas[1], "name"), "Lime pie");
        assert!(text(&metas[1], "description").contains("lemon"));
        assert_eq!(text(&metas[2], "id"), blank);
        assert_eq!(text(&metas[2], "name"), "Untitled");
        assert_eq!(text(&metas[2], "description"), "");
    });
}

#[test]
fn hands_notes_and_searches_over_to_the_app() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(60).await;
        let (_app, calls) = fx.fake_app().await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        let provider = provider_proxy(&conn).await;

        provider
            .activate_result("ID1", &["lemon", "tart"], 7)
            .await
            .unwrap();
        provider.launch_search(&["lime", "pie"], 7).await.unwrap();
        notes.open("ID2").await.unwrap();
        notes.quick_note().await.unwrap();

        let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            calls.take(),
            [
                (ACTION_OPEN_NOTE.to_owned(), strings(&["ID1", "lemon tart"])),
                (ACTION_SEARCH.to_owned(), strings(&["lime pie"])),
                (ACTION_OPEN_NOTE.to_owned(), strings(&["ID2", ""])),
                (ACTION_QUICK_NOTE.to_owned(), Vec::new()),
            ]
        );
    });
}

#[test]
fn sync_reindex_and_notes_dir() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        assert_eq!(
            notes.notes_dir().await.unwrap(),
            fx.notes().canonicalize().unwrap().display().to_string()
        );

        fx.write("synced.md", "Synced\n\nrambutan\n");
        notes.sync().await.unwrap();
        let rows = notes.search("rambutan", 0).await.unwrap();
        assert_eq!(ids(&rows), [provisional("synced.md")]);

        let mut changed = notes.receive_changed().await.unwrap();
        notes.reindex().await.unwrap();
        let signal = next(&mut changed, TIMEOUT).await.expect("Changed");
        assert!(signal.args().unwrap().ids.is_empty());
        let rows = notes.search("rambutan", 0).await.unwrap();
        assert_eq!(ids(&rows), [provisional("synced.md")]);
    });
}

#[test]
fn limits_are_capped() {
    let Some(mut fx) = Fixture::new() else { return };
    for n in 0..25 {
        fx.write(&format!("n{n}.md"), &format!("Note {n}\n\ncherry\n"));
    }
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        assert_eq!(notes.search("cherry", 0).await.unwrap().len(), 20);
        assert_eq!(notes.search("cherry", 3).await.unwrap().len(), 3);
        assert_eq!(notes.recent(u32::MAX).await.unwrap().len(), 25);
        assert_eq!(notes.search("", 5).await.unwrap().len(), 5);
    });
}

#[test]
fn a_second_instance_exits_quietly() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(60).await;
        let second = fx.command().output().unwrap();
        assert!(second.status.success(), "{second:?}");
        assert!(fx.service().try_wait().unwrap().is_none());
        assert!(fx.stop().await.success());
    });
}

#[test]
fn exits_when_idle_unless_the_app_runs() {
    let Some(mut fx) = Fixture::new() else { return };
    zbus::block_on(async {
        fx.start(1).await;
        let status = fx.wait_exit(TIMEOUT).await.expect("no idle exit");
        assert!(status.success(), "{status}");

        let (app, _calls) = fx.fake_app().await;
        fx.start(1).await;
        sleep(Duration::from_millis(2500)).await;
        assert!(
            fx.service().try_wait().unwrap().is_none(),
            "exited while the app runs"
        );
        app.release_name(sheaf_searchd::api::APP_NAME)
            .await
            .unwrap();
        let status = fx.wait_exit(TIMEOUT).await.expect("no idle exit");
        assert!(status.success(), "{status}");
    });
}

/// Whether process `pid` has a file of the index under `cache` mapped.
fn maps_the_index(pid: u32, cache: &std::path::Path) -> bool {
    let maps = fs::read_to_string(format!("/proc/{pid}/maps")).unwrap();
    maps.lines()
        .any(|line| line.contains(&*cache.to_string_lossy()))
}

#[test]
fn lets_go_of_the_index_while_idle() {
    let Some(mut fx) = Fixture::new() else { return };
    fx.write("figs.md", "Figs\n\nRipe figs from the market.\n");
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        assert_eq!(notes.search("market", 0).await.unwrap().len(), 1);
        let pid = fx.service().id();
        assert!(
            maps_the_index(pid, &fx.cache()),
            "no index mapped after a search"
        );

        let released = eventually(async || (!maps_the_index(pid, &fx.cache())).then_some(())).await;
        assert!(released.is_some(), "the index is still mapped while idle");

        fx.write("dates.md", "Dates\n\nSweet dates from the market.\n");
        let found = eventually(async || {
            let rows = notes.search("market", 0).await.unwrap();
            (rows.len() == 2).then_some(())
        })
        .await;
        assert!(found.is_some(), "a change while idle is not found");
    });
}

#[test]
fn catches_up_with_changes_made_while_stopped() {
    let Some(mut fx) = Fixture::new() else { return };
    fx.write("keep.md", "Keep\n\nolive\n");
    fx.write("change.md", "Change\n\nplum\n");
    fx.write("remove.md", "Remove\n\ndamson\n");
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        let notes = notes_proxy(&conn).await;
        assert_eq!(notes.search("plum", 0).await.unwrap().len(), 1);
        assert!(fx.stop().await.success(), "{}", fx.log());

        fx.write("change.md", "Change\n\nnectarine\n");
        let later = SystemTime::now() + Duration::from_secs(10);
        fs::File::options()
            .write(true)
            .open(fx.notes().join("change.md"))
            .unwrap()
            .set_modified(later)
            .unwrap();
        fs::remove_file(fx.notes().join("remove.md")).unwrap();
        fx.write("new.md", "New\n\nmedlar\n");

        fx.start(60).await;
        notes.sync().await.unwrap();
        assert!(notes.search("plum", 0).await.unwrap().is_empty());
        assert_eq!(
            ids(&notes.search("nectarine", 0).await.unwrap()),
            [provisional("change.md")]
        );
        assert!(notes.search("damson", 0).await.unwrap().is_empty());
        assert_eq!(
            ids(&notes.search("medlar", 0).await.unwrap()),
            [provisional("new.md")]
        );
        assert_eq!(notes.recent(0).await.unwrap().len(), 3);
        assert!(fx.log().contains("indexing 2 changed notes, removing 1"));
    });
}

#[test]
fn empties_the_trash_of_old_notes_on_start() {
    let Some(mut fx) = Fixture::new() else { return };
    fx.write(
        ".trash/old.md",
        "---\ndeleted: 2020-01-01T00:00:00+00:00\nfrom: \"\"\n---\nOld\n",
    );
    let recently = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24);
    fx.write(
        ".trash/recent.md",
        &format!("---\ndeleted: {recently}\nfrom: \"\"\n---\nRecent\n"),
    );
    zbus::block_on(async {
        fx.start(60).await;
        let conn = fx.connect().await;
        notes_proxy(&conn).await.sync().await.unwrap();
        assert!(!fx.notes().join(".trash/old.md").exists());
        assert!(fx.notes().join(".trash/recent.md").exists());
    });
}

#[test]
fn reindex_flag_rebuilds_without_dbus() {
    let Some(fx) = Fixture::new() else { return };
    fx.write("one.md", "One\n");
    fx.write("two.md", "Two\n");
    let output = fx
        .command()
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .arg("--reindex")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let env = Env {
        cache_home: Some(fx.cache()),
        ..Env::default()
    };
    let index_dir = env.index_dir(&fx.notes()).unwrap();
    let index = SearchIndex::open_read_only(&index_dir).unwrap().unwrap();
    assert_eq!(index.reader().count(), 2);

    let index = SearchIndex::open_or_create(&index_dir).unwrap();
    let _writer = index.writer().unwrap();
    let output = fx.command().arg("--reindex").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("locked"), "{stderr}");
}

#[test]
fn command_line_flags() {
    let Some(fx) = Fixture::new() else { return };
    let run = |arg: &str| fx.command().arg(arg).output().unwrap();
    let version = run("--version");
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("sheaf-searchd "));
    let help = run("--help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--reindex"));
    assert_eq!(run("--bogus").status.code(), Some(2));
}

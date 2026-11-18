//! Every command of the `sheaf` binary against a temporary notes folder.

#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::Sandbox;
use insta::assert_snapshot;
use serde_json::Value;

const EINKAUF: &str = "Einkauf";

fn script(sandbox: &Sandbox, name: &str, body: &str) -> PathBuf {
    let path = sandbox.dir().join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

// ---- without a subcommand, help, completions --------------------------------------

#[test]
fn without_a_subcommand_it_is_the_app_which_needs_a_display() {
    // The sandbox has no session bus and no display: the launch is the only instance
    // and has nowhere to put a window. With a display it would open one.
    let sandbox = Sandbox::empty();
    for args in [&[][..], &["--new"], &["--quick"]] {
        let run = sandbox.run(args);
        assert_eq!(run.code, 1, "{args:?}");
        assert_eq!(run.stdout, "");
        assert_eq!(
            run.stderr,
            "sheaf: cannot open a window: neither WAYLAND_DISPLAY nor DISPLAY is set\n"
        );
    }
    assert!(!sandbox.notes.join(".sheaf").exists());

    // JSON is what commands print; the app has none.
    let run = sandbox.run(&["--json"]);
    assert_eq!(run.code, 2);
    assert_snapshot!("no_subcommand_json", sandbox.document(&run.stderr, "error"));

    let run = sandbox.run(&["--new", "list"]);
    assert_eq!(run.code, 2);
    assert_eq!(
        run.stderr,
        "sheaf: --new and --quick start the app and take no command\n"
    );
    assert_eq!(sandbox.run(&["--new", "--quick"]).code, 2);
}

#[test]
fn completions_are_written_for_bash_zsh_and_fish() {
    let sandbox = Sandbox::empty();
    for shell in ["bash", "zsh", "fish"] {
        let script = sandbox.ok(&["completions", shell]);
        assert!(script.len() > 200, "{shell}: {script}");
        assert!(script.contains("sheaf"), "{shell}");
        assert!(script.contains("reindex"), "{shell}");
    }
    assert_eq!(sandbox.run(&["completions", "powershell"]).code, 2);
}

#[test]
fn help_json_describes_every_command_and_its_schema() {
    let sandbox = Sandbox::empty();
    let text = sandbox.ok(&["help", "--json"]);
    let help = parse(&text);
    sandbox.document(&text, "help");

    let commands = help["commands"].as_array().unwrap();
    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    let expected = [
        "new",
        "append",
        "prepend",
        "show",
        "edit",
        "search",
        "list",
        "tasks",
        "check",
        "uncheck",
        "tags",
        "folders",
        "mv",
        "rm",
        "restore",
        "pin",
        "unpin",
        "trash",
        "open",
        "quick",
        "reindex",
        "path",
        "completions",
        "help",
    ];
    assert_eq!(names, expected);

    for command in commands {
        let name = command["name"].as_str().unwrap();
        match command["output_schema"].as_str() {
            Some(file) => assert!(common::schema_dir().join(file).is_file(), "{name}: {file}"),
            None => assert_eq!(name, "completions"),
        }
    }
    let exit_codes: Vec<u64> = help["exit_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["code"].as_u64().unwrap())
        .collect();
    assert_eq!(exit_codes, [0, 1, 2, 3, 4]);
    assert!(
        common::schema_dir()
            .join(help["error_schema"].as_str().unwrap())
            .is_file()
    );

    let new = commands.iter().find(|c| c["name"] == "new").unwrap();
    let tag = new["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["long"] == "tag")
        .unwrap();
    assert_eq!(tag["repeatable"], true);
    let search = commands.iter().find(|c| c["name"] == "search").unwrap();
    assert_eq!(search["arguments"][0]["multiple"], true);
    assert_eq!(search["options"][0]["default"], "20");
}

#[test]
fn help_prints_usage_of_a_command_for_people() {
    let sandbox = Sandbox::empty();
    let help = sandbox.ok(&["help", "check"]);
    assert!(help.contains("Usage: sheaf check"), "{help}");
    assert!(help.contains("NUMBER|TEXT"), "{help}");
    assert_eq!(sandbox.run(&["help", "nonsense"]).code, 2);
}

// ---- exit codes and errors --------------------------------------------------------

#[test]
fn errors_have_fixed_exit_codes_and_a_json_shape() {
    let sandbox = Sandbox::seeded();
    assert_eq!(sandbox.run(&["list"]).code, 0);

    let invalid = sandbox.run(&["mv", EINKAUF, "../outside", "--json"]);
    assert_eq!(invalid.code, 1);
    assert_snapshot!("error_invalid", sandbox.document(&invalid.stderr, "error"));

    let usage = sandbox.run(&["list", "--limit", "many", "--json"]);
    assert_eq!(usage.code, 2);
    assert_snapshot!("error_usage", sandbox.document(&usage.stderr, "error"));

    let missing = sandbox.run(&["show", "No such note", "--json"]);
    assert_eq!(missing.code, 3);
    assert_eq!(missing.stdout, "");
    assert_snapshot!(
        "error_not_found",
        sandbox.document(&missing.stderr, "error")
    );

    let ambiguous = sandbox.run(&["show", "Ideen", "--json"]);
    assert_eq!(ambiguous.code, 4);
    assert_snapshot!(
        "error_ambiguous",
        sandbox.document(&ambiguous.stderr, "error")
    );
}

#[test]
fn errors_without_json_are_one_line_and_candidates_follow_ambiguity() {
    let sandbox = Sandbox::seeded();
    let missing = sandbox.run(&["show", "No such note"]);
    assert_eq!((missing.code, missing.stdout.as_str()), (3, ""));
    assert_eq!(missing.stderr, "sheaf: not found: No such note\n");

    let ambiguous = sandbox.run(&["show", "Ideen"]);
    assert_eq!(ambiguous.code, 4);
    assert_snapshot!(
        "error_ambiguous_human",
        sandbox.normalise(&ambiguous.stderr, true)
    );

    let usage = sandbox.run(&["bogus"]);
    assert_eq!(usage.code, 2);
    assert!(
        usage.stderr.contains("unrecognized subcommand"),
        "{}",
        usage.stderr
    );
}

#[test]
fn a_missing_text_is_a_usage_error_and_never_waits_for_a_terminal() {
    let sandbox = Sandbox::seeded();
    for args in [
        &["new"][..],
        &["append", EINKAUF],
        &["prepend", EINKAUF, "  "],
    ] {
        let run = sandbox.run(args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.stderr);
        assert!(
            run.stderr.starts_with("sheaf: no text given"),
            "{}",
            run.stderr
        );
    }
}

// ---- reading ----------------------------------------------------------------------

#[test]
fn path_prints_the_notes_folder() {
    let sandbox = Sandbox::empty();
    assert_eq!(
        sandbox.ok(&["path"]),
        format!("{}\n", sandbox.notes.display())
    );
    assert_snapshot!("path", sandbox.json(&["path"], "path"));
}

#[test]
fn list_json() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("list", sandbox.json(&["list"], "note-list"));
}

#[test]
fn list_human_is_one_aligned_line_per_note() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("list_human", sandbox.text(&["list"]));
}

#[test]
fn list_filters_sorts_and_limits() {
    let sandbox = Sandbox::seeded();
    let titles = |args: &[&str]| -> Vec<String> {
        let value = parse(&sandbox.json(args, "note-list"));
        value["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["title"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(titles(&["list", "--pinned"]), ["Einkauf"]);
    assert_eq!(
        titles(&["list", "--tag", "projekt", "--sort", "title"]),
        ["Besprechung Ausschreibung", "Project plan"]
    );
    assert_eq!(titles(&["list", "--tag", "#books"]), ["Reading list"]);
    assert_eq!(
        titles(&["list", "--folder", "Arbeit"]),
        ["Project plan", "Besprechung Ausschreibung"]
    );
    assert_eq!(titles(&["list", "--folder", "Planung"]), ["Project plan"]);
    assert_eq!(
        titles(&["list", "--limit", "2"]),
        ["Reading list", "Ideen für die Reise"]
    );
    // A note without `created:` is as old as its file.
    assert_eq!(
        titles(&["list", "--sort", "created", "--limit", "2"]),
        ["Reading list", "Ideen für die Reise"]
    );
    assert_eq!(
        titles(&["list", "--sort", "title", "--limit", "2"]),
        ["Besprechung Ausschreibung", "Einkauf"]
    );
    // The seeds were modified on 2026-11-05 and later.
    assert_eq!(
        titles(&["list", "--modified-since", "2999-01-01"]),
        Vec::<String>::new()
    );
    assert_eq!(
        titles(&["list", "--modified-since", "2000-01-01", "--limit", "1"]),
        ["Reading list"]
    );
    assert_eq!(
        titles(&[
            "list",
            "--modified-since",
            "2000-01-01T00:00:00+01:00",
            "--pinned"
        ]),
        ["Einkauf"]
    );

    let bad = sandbox.run(&["list", "--modified-since", "yesterday"]);
    assert_eq!(bad.code, 2, "{}", bad.stderr);
}

#[test]
fn search_json() {
    let sandbox = Sandbox::seeded();
    let text = sandbox.json(&["search", "ausschreibung"], "search");
    assert_snapshot!("search", text);
    let hits = parse(&text);
    let hits = hits["hits"].as_array().unwrap();
    assert_eq!(hits[0]["title"], "Besprechung Ausschreibung");
}

#[test]
fn search_scores_are_positive_and_descending() {
    let sandbox = Sandbox::seeded();
    let stdout = sandbox.ok(&["search", "ideen", "--json"]);
    let scores: Vec<f64> = parse(&stdout)["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| hit["score"].as_f64().unwrap())
        .collect();
    assert!(scores.len() >= 2, "{stdout}");
    assert!(scores.iter().all(|score| *score > 0.0));
    assert!(
        scores.windows(2).all(|pair| pair[0] >= pair[1]),
        "{scores:?}"
    );
}

#[test]
fn search_human_has_the_snippet_on_a_second_indented_line() {
    let sandbox = Sandbox::seeded();
    let text = sandbox.text(&["search", "Lissabon"]);
    assert_snapshot!("search_human", text);
    assert!(
        !text.contains('\x1b'),
        "no colour unless stdout is a terminal"
    );
}

#[test]
fn search_understands_filters_and_limits() {
    let sandbox = Sandbox::seeded();
    let ids = |query: &[&str]| -> Vec<String> {
        let args: Vec<&str> = ["search"].iter().chain(query).copied().collect();
        let value = parse(&sandbox.json(&args, "search"));
        value["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["title"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        ids(&["#projekt", "has:table"]),
        ["Besprechung Ausschreibung"]
    );
    assert_eq!(ids(&["open:", "folder:Arbeit"]), ["Project plan"]);
    assert_eq!(ids(&["pinned:"]), ["Einkauf"]);
    assert_eq!(ids(&["has:checklist"]).len(), 2);
    assert_eq!(ids(&["nonexistentword"]), Vec::<String>::new());
    let limited = parse(&sandbox.json(&["search", "has:checklist", "--limit", "1"], "search"));
    assert_eq!(
        (
            limited["hits"].as_array().unwrap().len(),
            limited["total"].as_u64()
        ),
        (1, Some(2))
    );
    assert_snapshot!(
        "search_filters",
        sandbox.json(&["search", "#projekt", "folder:Arbeit"], "search")
    );
}

#[test]
fn tasks_lists_checklist_items_of_all_notes_or_one() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("tasks", sandbox.json(&["tasks"], "tasks"));
    assert_snapshot!("tasks_open", sandbox.json(&["tasks", "--open"], "tasks"));
    assert_snapshot!(
        "tasks_of_note",
        sandbox.json(&["tasks", "--note", "Project plan"], "tasks")
    );
    assert_snapshot!("tasks_human", sandbox.text(&["tasks", "--open"]));
}

#[test]
fn tags_and_folders_are_counted() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("tags", sandbox.json(&["tags"], "tags"));
    assert_snapshot!("folders", sandbox.json(&["folders"], "folders"));
    assert_snapshot!("tags_human", sandbox.text(&["tags"]));
    assert_snapshot!("folders_human", sandbox.text(&["folders"]));
}

#[test]
fn show_prints_md_text_and_json() {
    let sandbox = Sandbox::seeded();
    let md = sandbox.ok(&["show", "Besprechung Ausschreibung"]);
    assert!(md.starts_with("# Besprechung Ausschreibung\n"), "{md}");
    assert!(!md.contains("id:"), "frontmatter must not be printed");
    assert_snapshot!("show_md", md);

    let text = sandbox.ok(&["show", "Besprechung", "--format", "text"]);
    assert!(!text.contains('#') || text.contains("#projekt"), "{text}");
    assert_snapshot!("show_text", text);

    assert_snapshot!("show_json", sandbox.json(&["show", "Besprechung"], "show"));
    // `--format json` is `--json`.
    let by_format = sandbox.ok(&["show", "Besprechung", "--format", "json"]);
    assert_eq!(
        sandbox.document(&by_format, "show"),
        sandbox.json(&["show", "Besprechung"], "show")
    );
}

#[test]
fn notes_are_found_by_id_title_and_prefix_without_case() {
    let sandbox = Sandbox::seeded();
    let title = |reference: &str| {
        parse(&sandbox.json(&["show", reference], "show"))["note"]["title"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(title("01JD3K8Z6Q4X7V9B2M5N8P0R3T"), "Einkauf");
    assert_eq!(title("01jd3k8z6q4x7v9b2m5n8p0r3t"), "Einkauf");
    assert_eq!(title("einkauf"), "Einkauf");
    assert_eq!(title("project"), "Project plan");
    assert_eq!(title("Ideen für den"), "Ideen für den Garten");
}

// ---- writing ----------------------------------------------------------------------

#[test]
fn new_creates_a_note_from_an_argument() {
    let sandbox = Sandbox::empty();
    let id = sandbox.ok(&["new", "Erste Notiz\nmit Text"]);
    assert_eq!(id.trim().len(), 26);
    assert!(sandbox.read("Erste Notiz.md").contains("mit Text"));
    assert_eq!(sandbox.ok(&["show", id.trim()]), "Erste Notiz\nmit Text\n");
    assert_snapshot!("new", sandbox.json(&["new", "Zweite Notiz"], "note"));
}

#[test]
fn new_reads_stdin_without_an_argument_and_with_a_dash() {
    let sandbox = Sandbox::empty();
    sandbox.ok_with(&["new"], "Aus stdin\nzweite Zeile\n");
    assert_eq!(
        sandbox.read("Aus stdin.md").lines().last(),
        Some("zweite Zeile")
    );
    sandbox.ok_with(&["new", "-"], "Mit Strich\n");
    assert!(sandbox.exists("Mit Strich.md"));
    assert_snapshot!(
        "new_stdin",
        sandbox.json_with(&["new"], "Von Skript\n- [ ] eins\n", "note")
    );
}

#[test]
fn new_takes_folder_tags_and_pin() {
    let sandbox = Sandbox::empty();
    let text = sandbox.json(
        &[
            "new",
            "Wichtig #inline",
            "--folder",
            "Arbeit/Neu",
            "--tag",
            "a",
            "--tag",
            "#b",
            "--pin",
        ],
        "note",
    );
    assert_snapshot!("new_options", text);
    assert!(
        sandbox.exists("Arbeit/Neu/Wichtig #inline.md")
            || sandbox.exists("Arbeit/Neu/Wichtig #inline.md".replace('#', "-").as_str())
    );
    let root = sandbox.ok(&["new", "Im Wurzelordner", "--folder", "/"]);
    assert_eq!(root.trim().len(), 26);
    assert!(sandbox.exists("Im Wurzelordner.md"));
    assert_eq!(sandbox.run(&["new", "x", "--tag", "not a tag"]).code, 1);
}

#[test]
fn new_with_a_template_puts_the_rest_of_the_text_after_it() {
    let sandbox = Sandbox::seeded();
    let text = sandbox.json(
        &[
            "new",
            "Jour fixe\nPunkt 1\nPunkt 2",
            "--template",
            "Meeting",
        ],
        "note",
    );
    assert_snapshot!("new_template", text);
    assert_eq!(
        sandbox.read("Jour fixe.md").split("---\n").last().unwrap(),
        "# Jour fixe\n\nAttendees:\n\n- [ ] Send minutes\nPunkt 1\nPunkt 2\n"
    );
    // The template alone is a note, with an empty title.
    sandbox.ok(&["new", "Nur Vorlage", "--template", "Meeting"]);
    let missing = sandbox.run(&["new", "x", "--template", "Gibt es nicht", "--json"]);
    assert_eq!(missing.code, 3);
    assert_eq!(
        sandbox.run(&["new", "x", "--template", "../config"]).code,
        1
    );
}

#[test]
fn append_and_prepend_take_text_from_the_argument_or_stdin() {
    let sandbox = Sandbox::seeded();
    sandbox.ok(&["append", "Ideen für den Garten", "Bohnen"]);
    sandbox.ok_with(&["append", "Ideen für den Garten"], "Gurken\nKürbis\n");
    sandbox.ok(&["prepend", "ideen für den", "Wichtig!"]);
    sandbox.ok_with(&["prepend", "ideen für den", "-"], "Zuerst\n");
    assert_snapshot!(
        "append_prepend",
        sandbox.ok(&["show", "Ideen für den Garten"])
    );
    assert_snapshot!(
        "append",
        sandbox.json(&["append", "Einkauf", "- [ ] Käse"], "note")
    );
    sandbox.ok(&["append", "--json", "Einkauf", "-- nach dem Strich"]);
    assert!(
        sandbox
            .read("Einkauf.md")
            .contains("- [ ] Käse\n-- nach dem Strich\n")
    );
    assert_snapshot!(
        "prepend",
        sandbox.json(&["prepend", "Einkauf", "Nicht vergessen"], "note")
    );
}

#[test]
fn writing_a_note_without_an_id_gives_it_a_real_one() {
    let sandbox = Sandbox::seeded();
    let before = sandbox.value(&["show", "Reading list"], "show");
    assert!(before["note"]["id"].as_str().unwrap().starts_with('p'));
    let after = sandbox.value(&["append", "Reading list", "Dune"], "note");
    let id = after["note"]["id"].as_str().unwrap();
    assert!(!id.starts_with('p'), "{id}");
    assert!(sandbox.read("Reading list.md").starts_with("---\nid: "));
}

#[test]
fn check_and_uncheck_by_number_and_by_text() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!(
        "check_number",
        sandbox.json(&["check", "Einkauf", "1"], "note")
    );
    assert!(sandbox.read("Einkauf.md").contains("- [x] Milch"));
    sandbox.ok(&["check", "Einkauf", "eier"]);
    assert!(sandbox.read("Einkauf.md").contains("- [x] Eier"));
    assert_snapshot!(
        "uncheck_text",
        sandbox.json(&["uncheck", "Einkauf", "Brot"], "note")
    );
    assert!(sandbox.read("Einkauf.md").contains("- [ ] Brot"));
    sandbox.ok(&["uncheck", "Einkauf", "2"]);
    // Checking a checked item changes nothing.
    sandbox.ok(&["check", "Einkauf", "Milch"]);

    let missing = sandbox.run(&["check", "Einkauf", "Wein"]);
    assert_eq!(missing.code, 3);
    let none = sandbox.run(&["check", "Einkauf", "9"]);
    assert_eq!(none.code, 3);
    let zero = sandbox.run(&["check", "Einkauf", "0"]);
    assert_eq!(zero.code, 3, "0 is text, and no item contains it");
    let ambiguous = sandbox.run(&["check", "Einkauf", "i", "--json"]);
    assert_eq!(ambiguous.code, 4);
    assert_snapshot!(
        "check_ambiguous",
        sandbox.document(&ambiguous.stderr, "error")
    );
}

#[test]
fn mv_moves_to_a_folder_and_to_the_root() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!(
        "mv",
        sandbox.json(&["mv", "Einkauf", "Archiv/2026"], "note")
    );
    assert!(sandbox.exists("Archiv/2026/Einkauf.md") && !sandbox.exists("Einkauf.md"));
    sandbox.ok(&["mv", "Einkauf", "/"]);
    assert!(sandbox.exists("Einkauf.md"));
    sandbox.ok(&["mv", "Einkauf", "Archiv"]);
    sandbox.ok(&["mv", "Einkauf", "."]);
    assert!(sandbox.exists("Einkauf.md"));
    assert_eq!(sandbox.run(&["mv", "Einkauf", ".trash"]).code, 1);
}

#[test]
fn pin_and_unpin() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("pin", sandbox.json(&["pin", "Reading list"], "note"));
    assert!(sandbox.read("Reading list.md").contains("pinned: true"));
    assert_snapshot!("unpin", sandbox.json(&["unpin", "Einkauf"], "note"));
    assert!(!sandbox.read("Einkauf.md").contains("pinned"));
    let pinned = sandbox.json(&["list", "--pinned"], "note-list");
    assert!(pinned.contains("Reading list") && !pinned.contains("Einkauf"));
}

#[test]
fn rm_moves_to_the_trash_and_restore_brings_the_note_back() {
    let sandbox = Sandbox::seeded();
    assert_snapshot!("trash_empty", sandbox.json(&["trash"], "trash"));

    assert_snapshot!("rm", sandbox.json(&["rm", "Project plan"], "note"));
    assert!(!sandbox.exists("Arbeit/Planung/Project plan.md"));
    assert!(sandbox.exists(".trash/Project plan.md"), "rm never deletes");
    assert_eq!(sandbox.run(&["show", "Project plan"]).code, 3);

    assert_snapshot!("trash", sandbox.json(&["trash"], "trash"));
    assert_snapshot!("trash_human", sandbox.text(&["trash"]));

    // Restore finds the note in the trash by id, exact title or unique prefix.
    assert_snapshot!("restore", sandbox.json(&["restore", "proj"], "note"));
    assert!(
        sandbox.exists("Arbeit/Planung/Project plan.md")
            && !sandbox.exists(".trash/Project plan.md")
    );
    assert!(sandbox.json(&["trash"], "trash").contains("\"notes\": []"));
    assert_eq!(sandbox.run(&["restore", "Project plan"]).code, 3);

    let id = sandbox.ok(&["rm", "Reading list"]);
    sandbox.ok(&["restore", id.trim()]);
    assert!(sandbox.exists("Reading list.md"));
}

#[test]
fn restore_reports_ambiguous_titles_with_candidates() {
    let sandbox = Sandbox::seeded();
    sandbox.ok(&["rm", "Ideen für den Garten"]);
    sandbox.ok(&["rm", "Ideen für die Reise"]);
    let run = sandbox.run(&["restore", "Ideen", "--json"]);
    assert_eq!(run.code, 4);
    assert_eq!(
        parse(&run.stderr)["error"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn edit_runs_the_editor_and_renames_the_file_after_the_title() {
    let sandbox = Sandbox::seeded();
    let editor = script(
        &sandbox,
        "editor.sh",
        "sed -i 's/^Einkauf$/Wocheneinkauf/' \"$1\"\nprintf 'Notiert\\n' >> \"$1\"",
    );
    let output = sandbox
        .command()
        .env("EDITOR", &editor)
        .args(["edit", "Einkauf", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_snapshot!("edit", sandbox.document(&stdout, "note"));
    assert!(sandbox.exists("Wocheneinkauf.md") && !sandbox.exists("Einkauf.md"));
    assert!(sandbox.read("Wocheneinkauf.md").ends_with("Notiert\n"));
    assert!(sandbox.read("Wocheneinkauf.md").contains("pinned: true"));
}

#[test]
fn edit_prefers_visual_and_passes_editor_arguments() {
    let sandbox = Sandbox::seeded();
    let visual = script(
        &sandbox,
        "visual.sh",
        "printf 'visual %s\\n' \"$1\" >> \"$2\"",
    );
    let editor = script(&sandbox, "editor.sh", "exit 7");
    let run = sandbox
        .command()
        .env("VISUAL", format!("{} first", visual.display()))
        .env("EDITOR", &editor)
        .args(["edit", "Ideen für den Garten"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        sandbox
            .read("Ideen für den Garten.md")
            .ends_with("visual first\n")
    );

    let failing = sandbox
        .command()
        .env("EDITOR", &editor)
        .args(["edit", "Einkauf"])
        .output()
        .unwrap();
    assert_eq!(failing.status.code(), Some(1));
}

#[test]
fn edit_without_an_editor_is_an_invalid_request() {
    let sandbox = Sandbox::seeded();
    let run = sandbox.run(&["edit", "Einkauf", "--json"]);
    assert_eq!(run.code, 1);
    assert_eq!(parse(&run.stderr)["error"]["code"], "invalid");
}

// ---- the app and the service --------------------------------------------------------

#[test]
fn open_and_quick_report_that_the_app_cannot_be_reached() {
    let sandbox = Sandbox::seeded();
    let open = sandbox.run(&["open", "Einkauf", "--json"]);
    assert_eq!(open.code, 1);
    assert_eq!(parse(&open.stderr)["error"]["code"], "app_unavailable");
    sandbox.document(&open.stderr, "error");
    let quick = sandbox.run(&["quick", "--json"]);
    assert_eq!(quick.code, 1);
    assert_eq!(parse(&quick.stderr)["error"]["code"], "app_unavailable");
    assert_eq!(sandbox.run(&["open", "No such note"]).code, 3);
}

/// A copy of the binary in a folder of its own, so that no `sheaf-searchd` of the build
/// directory sits next to it.
fn lonely_binary(sandbox: &Sandbox) -> PathBuf {
    let dir = sandbox.dir().join("bin");
    fs::create_dir(&dir).unwrap();
    let copy = dir.join("sheaf");
    fs::copy(env!("CARGO_BIN_EXE_sheaf"), &copy).unwrap();
    copy
}

fn reindex(sandbox: &Sandbox, binary: &Path, path: &str) -> common::Run {
    // Starting a program that was just written can fail while another test forks.
    let output = (0..50)
        .find_map(|_| {
            let result = sandbox
                .command_of(binary, path)
                .args(["reindex", "--json"])
                .output();
            match result {
                Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    None
                }
                other => Some(other.unwrap()),
            }
        })
        .unwrap();
    common::Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

#[test]
fn reindex_without_a_service_or_binary_is_service_unavailable() {
    let sandbox = Sandbox::seeded();
    let binary = lonely_binary(&sandbox);
    let empty = sandbox.dir().join("empty");
    let run = reindex(&sandbox, &binary, empty.to_str().unwrap());
    assert_eq!(run.code, 1);
    assert_eq!(parse(&run.stderr)["error"]["code"], "service_unavailable");
}

#[test]
fn reindex_runs_the_service_binary_next_to_it_or_in_path() {
    let sandbox = Sandbox::seeded();
    let binary = lonely_binary(&sandbox);
    let record = sandbox.dir().join("record");
    let body = format!(
        "echo \"$@ $SHEAF_NOTES_DIR\" > {}\necho noise",
        record.display()
    );

    // Next to the executable.
    let beside = script(&sandbox, "bin/sheaf-searchd", &body);
    let run = reindex(
        &sandbox,
        &binary,
        sandbox.dir().join("empty").to_str().unwrap(),
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_snapshot!("reindex", sandbox.document(&run.stdout, "done"));
    assert_eq!(
        fs::read_to_string(&record).unwrap(),
        format!("--reindex {}\n", sandbox.notes.display())
    );

    // In PATH.
    fs::remove_file(beside).unwrap();
    fs::remove_file(&record).unwrap();
    let in_path = sandbox.dir().join("tools");
    fs::create_dir(&in_path).unwrap();
    fs::rename(
        script(&sandbox, "sheaf-searchd", &body),
        in_path.join("sheaf-searchd"),
    )
    .unwrap();
    let run = reindex(&sandbox, &binary, in_path.to_str().unwrap());
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(record.is_file());

    // A failing service binary is reported.
    script(&sandbox, "tools/sheaf-searchd", "exit 3");
    let run = reindex(&sandbox, &binary, in_path.to_str().unwrap());
    assert_eq!(run.code, 1);
}

#[test]
fn messages_and_errors_are_in_the_language_of_the_user() {
    let locales = std::process::Command::new("locale").arg("-a").output();
    let has_german = locales.is_ok_and(|out| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|locale| locale.eq_ignore_ascii_case("de_DE.utf8"))
    });
    let sandbox = Sandbox::seeded();
    let messages = sandbox.notes.parent().unwrap().join("locale/de/LC_MESSAGES");
    fs::create_dir_all(&messages).unwrap();
    let compiled = std::process::Command::new("msgfmt")
        .arg("-o")
        .arg(messages.join("sheaf.mo"))
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/../../po/de.po"))
        .status();
    if !has_german || !compiled.is_ok_and(|status| status.success()) {
        eprintln!("skipping: no de_DE.UTF-8 locale or no msgfmt");
        return;
    }
    let german = |args: &[&str]| {
        let output = sandbox
            .command()
            .env("LC_ALL", "de_DE.UTF-8")
            .env("SHEAF_LOCALEDIR", messages.parent().unwrap().parent().unwrap())
            .args(args)
            .output()
            .unwrap();
        String::from_utf8(output.stderr).unwrap()
    };
    assert_eq!(
        german(&["show", "No such note"]),
        "sheaf: nicht gefunden: No such note\n"
    );
    assert_eq!(
        german(&["help", "bogus"]),
        "sheaf: unbekannter Befehl: bogus\n"
    );
}

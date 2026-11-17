//! A sandbox notes folder, a scrubbed environment and helpers shared by the CLI tests.
//!
//! Nothing here can reach the real notes, configuration, cache or session bus: the
//! binary runs with an empty environment plus paths into a temporary directory, and
//! without `DBUS_SESSION_BUS_ADDRESS`, so it always takes the in-memory index.

// Each test binary uses its own part of these helpers.
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use assert_cmd::Command;
use regex::{Captures, Regex};
use serde_json::Value;
use tempfile::TempDir;

pub struct Sandbox {
    root: TempDir,
    pub notes: PathBuf,
}

/// What a run of the binary printed.
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

const SEEDS: [(&str, &str); 7] = [
    (
        "Einkauf.md",
        "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R3T\ncreated: 2026-11-01T08:00:00+01:00\npinned: true\n---\n\
         Einkauf\n- [ ] Milch\n- [x] Brot\n- [ ] Eier\n\n#haushalt\n",
    ),
    (
        "Arbeit/Besprechung Ausschreibung.md",
        "---\nid: 01JD3K8Z7A4X7V9B2M5N8P0R3T\ncreated: 2026-11-02T09:30:00+01:00\n---\n\
         # Besprechung Ausschreibung\n\nDie Ausschreibung für das Projekt Frühling wurde besprochen. #projekt\n\n\
         | Phase | Frist |\n|---|---|\n| Angebot | 30. November |\n",
    ),
    (
        "Arbeit/Planung/Project plan.md",
        "---\nid: 01JD3K8Z7B4X7V9B2M5N8P0R3T\ncreated: 2026-11-03T10:00:00+01:00\n---\n\
         Project plan\n\nKickoff next week. #projekt\n\n- [x] Write the brief\n- [ ] Book the venue\n",
    ),
    (
        "Ideen für den Garten.md",
        "---\nid: 01JD3K8Z7C4X7V9B2M5N8P0R3T\ncreated: 2026-11-04T11:00:00+01:00\n---\n\
         Ideen für den Garten\n\nTomaten und Kräuter pflanzen.\n",
    ),
    (
        "Ideen für die Reise.md",
        "---\nid: 01JD3K8Z7D4X7V9B2M5N8P0R3T\ncreated: 2026-11-05T12:00:00+01:00\ntags: [urlaub]\n---\n\
         Ideen für die Reise\n\nNach Lissabon fahren.\n",
    ),
    // No frontmatter: an id derived from the path until the note is first written.
    (
        "Reading list.md",
        "Reading list\n\nThe Overstory and Braiding Sweetgrass. #books\n",
    ),
    (
        ".sheaf/templates/Meeting.md",
        "# {{title}}\n\nAttendees:\n\n- [ ] Send minutes\n",
    ),
];

const SEED_FOLDERS: [&str; 1] = ["Archiv"];

impl Sandbox {
    pub fn empty() -> Self {
        let root = tempfile::tempdir().unwrap();
        let notes = root.path().join("notes");
        for dir in ["notes", "home", "config", "cache", "run", "tmp", "empty"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        Sandbox { root, notes }
    }

    /// The fixed set of notes the snapshots are made of. File times rise in the order
    /// of `SEEDS`, so "most recently modified first" is the reverse of it.
    pub fn seeded() -> Self {
        let sandbox = Self::empty();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_000_000);
        for (index, (path, text)) in SEEDS.iter().enumerate() {
            sandbox.write(path, text);
            let file = File::options()
                .write(true)
                .open(sandbox.notes.join(path))
                .unwrap();
            file.set_modified(base + Duration::from_secs(60 * index as u64))
                .unwrap();
        }
        for folder in SEED_FOLDERS {
            fs::create_dir_all(sandbox.notes.join(folder)).unwrap();
        }
        sandbox
    }

    pub fn dir(&self) -> &Path {
        self.root.path()
    }

    pub fn write(&self, rel_path: &str, text: &str) {
        let path = self.notes.join(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn read(&self, rel_path: &str) -> String {
        fs::read_to_string(self.notes.join(rel_path)).unwrap()
    }

    pub fn exists(&self, rel_path: &str) -> bool {
        self.notes.join(rel_path).exists()
    }

    /// The sheaf command with the scrubbed environment, `--dir` and no input.
    pub fn command(&self) -> Command {
        self.command_of(
            Path::new(env!("CARGO_BIN_EXE_sheaf")),
            &std::env::var("PATH").unwrap(),
        )
    }

    /// Like [`Sandbox::command`] for a copy of the binary and a chosen `PATH`.
    pub fn command_of(&self, binary: &Path, path: &str) -> Command {
        let root = self.root.path();
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("TMPDIR", root.join("tmp"))
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .arg("--dir")
            .arg(&self.notes)
            .write_stdin(Vec::new());
        command
    }

    pub fn run(&self, args: &[&str]) -> Run {
        self.run_with(args, "")
    }

    pub fn run_with(&self, args: &[&str], stdin: &str) -> Run {
        let output = self
            .command()
            .args(args)
            .write_stdin(stdin)
            .output()
            .unwrap();
        Run {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }

    /// Runs a command that must succeed and returns what it printed.
    pub fn ok(&self, args: &[&str]) -> String {
        self.ok_with(args, "")
    }

    pub fn ok_with(&self, args: &[&str], stdin: &str) -> String {
        let run = self.run_with(args, stdin);
        assert_eq!(run.code, 0, "{args:?} failed: {}", run.stderr);
        assert_eq!(run.stderr, "", "{args:?} wrote to stderr");
        run.stdout
    }

    /// Runs `args --json`, checks the document against the schema and returns it,
    /// normalised for snapshots.
    pub fn json(&self, args: &[&str], schema: &str) -> String {
        self.json_with(args, "", schema)
    }

    pub fn json_with(&self, args: &[&str], stdin: &str, schema: &str) -> String {
        let args: Vec<&str> = args.iter().copied().chain(["--json"]).collect();
        let stdout = self.ok_with(&args, stdin);
        self.document(&stdout, schema)
    }

    /// Runs `args --json` and returns the document as it was printed, after checking it.
    pub fn value(&self, args: &[&str], schema: &str) -> Value {
        let args: Vec<&str> = args.iter().copied().chain(["--json"]).collect();
        check_document(&self.ok(&args), schema)
    }

    /// A JSON document that must be valid for `schema`, as normalised pretty text.
    pub fn document(&self, text: &str, schema: &str) -> String {
        check_document(text, schema);
        let value: Value = serde_json::from_str(&self.normalise(text, false)).unwrap();
        serde_json::to_string_pretty(&value).unwrap()
    }

    /// Replaces what differs between runs: ids (numbered by first appearance), times,
    /// scores and the sandbox paths. With `pad`, an id that starts a line keeps its width,
    /// so that tables stay aligned in snapshots.
    pub fn normalise(&self, text: &str, pad: bool) -> String {
        let mut text = text.replace(self.notes.to_str().unwrap(), "[notes]");
        text = text.replace(self.dir().to_str().unwrap(), "[sandbox]");
        let mut numbers: HashMap<String, usize> = HashMap::new();
        let id = Regex::new(r"\b(?:[0-9A-HJKMNP-TV-Z]{26}|p[0-9a-hjkmnp-tv-z]{25})\b").unwrap();
        let source = text.clone();
        text = id
            .replace_all(&source, |found: &Captures<'_>| {
                let next = numbers.len() + 1;
                let number = *numbers.entry(found[0].to_owned()).or_insert(next);
                let label = format!("[id{number}]");
                let line = source[..found.get(0).unwrap().start()]
                    .rsplit('\n')
                    .next()
                    .unwrap();
                match pad && line.trim().is_empty() {
                    true => format!("{label:<26}"),
                    false => label,
                }
            })
            .into_owned();
        let rfc3339 = r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z";
        text = Regex::new(rfc3339)
            .unwrap()
            .replace_all(&text, "[time]")
            .into_owned();
        text = Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}")
            .unwrap()
            .replace_all(&text, "[time]")
            .into_owned();
        Regex::new(r#""score":[0-9.e-]+"#)
            .unwrap()
            .replace_all(&text, r#""score":"[score]""#)
            .into_owned()
    }

    /// Plain text output, normalised.
    pub fn text(&self, args: &[&str]) -> String {
        self.normalise(&self.ok(args), true)
    }
}

fn check_document(text: &str, schema: &str) -> Value {
    assert!(
        text.starts_with("{\"schema\":1"),
        "schema is not the first key: {text}"
    );
    assert_eq!(text.lines().count(), 1, "not exactly one line: {text}");
    let value: Value = serde_json::from_str(text).unwrap();
    let problems = validate(schema, &value);
    assert!(problems.is_empty(), "{schema}: {problems:#?}\n{text}");
    value
}

/// Where the schema files live.
pub fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/cli-schema")
}

/// Checks `value` against `docs/cli-schema/<schema>.schema.json`. Understands what
/// those files use (`$ref`, `allOf`, `type`, `const`, `required`, `properties`,
/// `items`, `minimum`, `minItems`, `maxItems`) and, unlike the schemas themselves,
/// rejects keys they do not describe, so that a field cannot be added unnoticed.
pub fn validate(schema: &str, value: &Value) -> Vec<String> {
    let doc = load(&format!("{schema}.schema.json"));
    let mut problems = Vec::new();
    check(&doc, &doc, value, "$", true, &mut problems);
    problems
}

fn load(file: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(schema_dir().join(file)).unwrap()).unwrap()
}

/// The schema a `$ref` points to and the document it lives in.
fn resolve(doc: &Value, reference: &str) -> (Value, Value) {
    let (file, pointer) = reference.split_once('#').unwrap();
    let doc = if file.is_empty() {
        doc.clone()
    } else {
        load(file)
    };
    let target = doc.pointer(pointer).unwrap().clone();
    (doc, target)
}

fn check(
    doc: &Value,
    schema: &Value,
    value: &Value,
    at: &str,
    strict: bool,
    out: &mut Vec<String>,
) {
    if let Some(reference) = schema.get("$ref") {
        let (doc, schema) = resolve(doc, reference.as_str().unwrap());
        return check(&doc, &schema, value, at, strict, out);
    }
    for part in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        check(doc, part, value, at, false, out);
    }
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        out.push(format!("{at}: expected {expected}"));
    }
    if let Some(types) = schema.get("type") {
        let names: Vec<&str> = match types {
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            name => name.as_str().into_iter().collect(),
        };
        if !names.iter().any(|name| has_type(name, value)) {
            out.push(format!("{at}: {value} is not of type {names:?}"));
            return;
        }
    }
    if let (Some(minimum), Some(number)) = (schema.get("minimum"), value.as_f64())
        && number < minimum.as_f64().unwrap()
    {
        out.push(format!("{at}: {number} is below {minimum}"));
    }
    if let Some(object) = value.as_object() {
        for key in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if !object.contains_key(key.as_str().unwrap()) {
                out.push(format!("{at}: missing {key}"));
            }
        }
        for (key, property) in schema
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if let Some(found) = object.get(key) {
                check(doc, property, found, &format!("{at}.{key}"), true, out);
            }
        }
        if strict {
            let described = described_keys(doc, schema);
            for key in object.keys().filter(|key| !described.contains(*key)) {
                out.push(format!("{at}: {key} is not in the schema"));
            }
        }
    }
    if let Some(array) = value.as_array() {
        for (number, item) in array.iter().enumerate() {
            if let Some(items) = schema.get("items") {
                check(doc, items, item, &format!("{at}[{number}]"), true, out);
            }
        }
        let size = |key: &str| schema.get(key).and_then(Value::as_u64);
        if size("minItems").is_some_and(|min| (array.len() as u64) < min)
            || size("maxItems").is_some_and(|max| array.len() as u64 > max)
        {
            out.push(format!("{at}: {} items", array.len()));
        }
    }
}

fn described_keys(doc: &Value, schema: &Value) -> BTreeSet<String> {
    if let Some(reference) = schema.get("$ref") {
        let (doc, schema) = resolve(doc, reference.as_str().unwrap());
        return described_keys(&doc, &schema);
    }
    let own = schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten();
    let mut keys: BTreeSet<String> = own.map(|(key, _)| key.clone()).collect();
    for part in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        keys.extend(described_keys(doc, part));
    }
    keys
}

fn has_type(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => false,
    }
}

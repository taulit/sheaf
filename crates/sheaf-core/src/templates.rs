//! Note templates: Markdown files in `<notes>/.sheaf/templates/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::ZonedDateTime;

use crate::error::{Error, Result};
use crate::i18n::tr;
use crate::store::NOTE_EXT;

const TEMPLATES_DIR: &str = ".sheaf/templates";

/// The folder that holds the templates of the notes folder `root`.
pub fn dir(root: &Path) -> PathBuf {
    root.join(TEMPLATES_DIR)
}

/// The names of all templates (file names without `.md`), sorted.
pub fn list(root: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(dir(root)) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if let Ok(name) = entry.file_name().into_string()
            && let Some(stem) = name.strip_suffix(NOTE_EXT)
            && !name.starts_with('.')
            && entry.file_type()?.is_file()
        {
            names.push(stem.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The text of a new note made from the template `name`.
pub fn render(root: &Path, name: &str, title: &str, now: &ZonedDateTime) -> Result<String> {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return Err(Error::Invalid(format!("not a template name: {name}")));
    }
    match fs::read_to_string(dir(root).join(format!("{name}{NOTE_EXT}"))) {
        Ok(template) => Ok(expand(&template, title, now)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Err(Error::NotFound(name.to_owned())),
        Err(err) => Err(err.into()),
    }
}

/// The text of a new note made from the template `name` and the text `given` for it,
/// as the CLI and the app create it: the first line of `given` is the title, the rest
/// goes after the template.
pub fn note_text(root: &Path, name: &str, given: &str, now: &ZonedDateTime) -> Result<String> {
    let (title, rest) = given.split_once('\n').unwrap_or((given, ""));
    let mut text = render(root, name, title.trim(), now)?;
    if !rest.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(rest);
    }
    Ok(text)
}

/// Creates the templates folder of the notes folder `root` if it is missing and
/// returns it, for a file manager to show.
pub fn ensure_dir(root: &Path) -> Result<PathBuf> {
    let dir = dir(root);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Replaces `{{date}}` (the date in the order of the language, ISO in English),
/// `{{time}}` (`HH:MM`) and `{{title}}`. Other placeholders stay as they are.
pub fn expand(template: &str, title: &str, now: &ZonedDateTime) -> String {
    let mut out = String::with_capacity(template.len() + title.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(len) = rest[open..].find("}}") else {
            break;
        };
        let placeholder = &rest[open..open + len + 2];
        out.push_str(&rest[..open]);
        match placeholder[2..placeholder.len() - 2].trim() {
            "date" => out.push_str(&date(now)),
            "time" => out.push_str(&now.strftime("%H:%M").to_string()),
            "title" => out.push_str(title),
            _ => out.push_str(placeholder),
        }
        rest = &rest[open + placeholder.len()..];
    }
    out.push_str(rest);
    out
}

/// The date of `now` with two-digit day and month, in the order the translation gives.
fn date(now: &ZonedDateTime) -> String {
    // TRANSLATORS: the date that replaces {{date}} in a note template. Keep the words
    // in braces; German: "{day}.{month}.{year}".
    tr("{year}-{month}-{day}")
        .replace("{year}", &now.year().to_string())
        .replace("{month}", &format!("{:02}", now.month()))
        .replace("{day}", &format!("{:02}", now.day()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> ZonedDateTime {
        "2026-11-16T09:05:59+01:00[Europe/Berlin]".parse().unwrap()
    }

    #[test]
    fn expands_known_placeholders_only() {
        let cases = [
            (
                "# {{title}}\n\n{{date}} {{time}}\n",
                "# Besprechung\n\n2026-11-16 09:05\n",
            ),
            (
                "{{ title }}/{{DATE}}/{{ date }}",
                "Besprechung/{{DATE}}/2026-11-16",
            ),
            ("{{unknown}} {{}} {{title", "{{unknown}} {{}} {{title"),
            ("{{title}}{{title}}", "BesprechungBesprechung"),
            (
                "no placeholders, ümlauts }} {",
                "no placeholders, ümlauts }} {",
            ),
            ("", ""),
        ];
        for (template, expected) in cases {
            assert_eq!(
                expand(template, "Besprechung", &now()),
                expected,
                "{template:?}"
            );
        }
        // A title is text, not a template.
        assert_eq!(expand("{{title}}", "{{date}}", &now()), "{{date}}");
    }

    #[test]
    fn the_given_text_gives_the_title_and_follows_the_template() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir(root.path())).unwrap();
        fs::write(
            dir(root.path()).join("Protokoll.md"),
            "# {{title}}\n\nAm {{date}}",
        )
        .unwrap();
        let text = |given: &str| note_text(root.path(), "Protokoll", given, &now()).unwrap();
        assert_eq!(text(""), "# \n\nAm 2026-11-16");
        assert_eq!(text(" Jour fixe "), "# Jour fixe\n\nAm 2026-11-16");
        assert_eq!(
            text("Jour fixe\n- Punkt eins\n"),
            "# Jour fixe\n\nAm 2026-11-16\n- Punkt eins\n"
        );
        assert!(matches!(
            note_text(root.path(), "Fehlt", "x", &now()),
            Err(Error::NotFound(_))
        ));
        assert_eq!(ensure_dir(root.path()).unwrap(), dir(root.path()));
        let fresh = tempfile::tempdir().unwrap();
        assert!(ensure_dir(fresh.path()).unwrap().is_dir());
    }

    #[test]
    fn lists_and_renders_template_files() {
        let root = tempfile::tempdir().unwrap();
        assert!(list(root.path()).unwrap().is_empty());
        assert!(matches!(
            render(root.path(), "Protokoll", "x", &now()),
            Err(Error::NotFound(_))
        ));

        fs::create_dir_all(dir(root.path()).join("folder.md")).unwrap();
        for (name, text) in [
            ("Protokoll.md", "# {{title}}\n\nDatum: {{date}}\n"),
            ("Anruf.md", "{{time}} Anruf\n"),
            (".hidden.md", "x"),
            ("readme.txt", "x"),
        ] {
            fs::write(dir(root.path()).join(name), text).unwrap();
        }

        assert_eq!(list(root.path()).unwrap(), ["Anruf", "Protokoll"]);
        assert_eq!(
            render(root.path(), "Protokoll", "Jour fixe", &now()).unwrap(),
            "# Jour fixe\n\nDatum: 2026-11-16\n"
        );
        for name in ["", "../config", ".hidden", "a/b"] {
            assert!(
                matches!(
                    render(root.path(), name, "x", &now()),
                    Err(Error::Invalid(_))
                ),
                "{name:?}"
            );
        }
    }
}

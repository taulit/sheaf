//! Note templates: Markdown files in `<notes>/.sheaf/templates/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jiff::ZonedDateTime;

use crate::error::{Error, Result};
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

/// Replaces `{{date}}` (ISO date), `{{time}}` (`HH:MM`) and `{{title}}`. Other
/// placeholders stay as they are.
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
            "date" => out.push_str(&now.strftime("%Y-%m-%d").to_string()),
            "time" => out.push_str(&now.strftime("%H:%M").to_string()),
            "title" => out.push_str(title),
            _ => out.push_str(placeholder),
        }
        rest = &rest[open + placeholder.len()..];
    }
    out.push_str(rest);
    out
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

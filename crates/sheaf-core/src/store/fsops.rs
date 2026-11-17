//! File operations that never leave a half-written file and never overwrite one by
//! accident, plus the rules for file names.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::i18n::tr;

/// File names are cut to this many bytes, before the collision suffix and extension.
const MAX_STEM_BYTES: usize = 120;

/// Prefix of the temporary files of atomic writes. Hidden, so scans and the watcher skip them.
pub(crate) const TEMP_PREFIX: &str = ".sheaf-tmp-";

/// Replaces the content of `path` atomically, keeping the permissions of an existing file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = parent(path);
    let temp = write_temp(dir, bytes, Some(path))?;
    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    sync_dir(dir);
    Ok(())
}

/// Writes a new file `<stem><ext>` into `dir`. While the name is taken, `<sep>2`,
/// `<sep>3`, … is appended to the stem. Returns the name used.
pub(crate) fn write_new(
    dir: &Path,
    stem: &str,
    sep: &str,
    ext: &str,
    bytes: &[u8],
) -> io::Result<String> {
    let temp = write_temp(dir, bytes, None)?;
    let name = move_new(&temp, dir, stem, sep, ext);
    if name.is_err() {
        let _ = fs::remove_file(&temp);
    }
    name
}

/// Like [`write_new`], with the bytes read from `source` in pieces, so that a large
/// file is never in memory whole. Before each piece `go_on` is asked; if it says no,
/// the copy is given up and leaves nothing behind.
pub(crate) fn copy_new(
    dir: &Path,
    stem: &str,
    sep: &str,
    ext: &str,
    source: &mut dyn Read,
    go_on: &dyn Fn() -> bool,
) -> io::Result<String> {
    let temp = dir.join(format!("{TEMP_PREFIX}{}", ulid::Ulid::generate()));
    let copied = copy_to(&temp, source, go_on);
    let name = copied.and_then(|()| move_new(&temp, dir, stem, sep, ext));
    if name.is_err() {
        let _ = fs::remove_file(&temp);
    }
    name
}

fn copy_to(temp: &Path, source: &mut dyn Read, go_on: &dyn Fn() -> bool) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    let mut piece = vec![0; 256 * 1024];
    loop {
        if !go_on() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "the copy was given up",
            ));
        }
        let read = match source.read(&mut piece) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        file.write_all(&piece[..read])?;
    }
    file.sync_all()
}

/// Moves `from` to `<dir>/<stem><ext>`, numbered like [`write_new`] while the name is
/// taken. A file that already has one of the tried names stays where it is.
pub(crate) fn move_new(
    from: &Path,
    dir: &Path,
    stem: &str,
    sep: &str,
    ext: &str,
) -> io::Result<String> {
    for n in 1..=u32::MAX {
        let name = if n == 1 {
            format!("{stem}{ext}")
        } else {
            format!("{stem}{sep}{n}{ext}")
        };
        let to = dir.join(&name);
        if to == from || place(from, &to)? {
            sync_dir(dir);
            return Ok(name);
        }
    }
    Err(io::Error::other("no free file name"))
}

/// Moves `from` to `to` unless `to` exists; returns whether it moved.
fn place(from: &Path, to: &Path) -> io::Result<bool> {
    // A hard link fails if the target exists, which a rename would silently replace.
    match fs::hard_link(from, to) {
        Ok(()) => {
            fs::remove_file(from)?;
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        // No hard links on this file system (FAT, some network mounts).
        Err(_) if to.symlink_metadata().is_ok() => Ok(false),
        Err(_) => fs::rename(from, to).map(|()| true),
    }
}

fn write_temp(dir: &Path, bytes: &[u8], like: Option<&Path>) -> io::Result<PathBuf> {
    let temp = dir.join(format!("{TEMP_PREFIX}{}", ulid::Ulid::generate()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = file.write_all(bytes).and_then(|()| {
        if let Some(metadata) = like.and_then(|path| fs::metadata(path).ok()) {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()
    });
    match written {
        Ok(()) => Ok(temp),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

/// Makes a rename durable. Not every file system can sync a directory, so errors are ignored.
fn sync_dir(dir: &Path) {
    if let Ok(dir) = File::open(dir) {
        let _ = dir.sync_all();
    }
}

fn parent(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("."))
}

/// The file name (without extension) for a note with this title.
pub(crate) fn file_stem_for(title: &str) -> String {
    // Syncthing and Nextcloud carry notes to Android and Windows, whose file systems
    // reject these characters and names ending in a dot.
    let portable: String = title
        .chars()
        .filter(|c| !matches!(c, '?' | '*' | '"' | '<' | '>' | '|'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let portable = portable.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = clean_name(&portable, |c| match c {
        '/' | '\\' | ':' => '-',
        c => c,
    });
    let stem = stem.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        tr("Untitled")
    } else {
        stem
    }
}

/// Stem and extension (with its dot, or empty) for an attachment called `file_name`.
/// Only characters that need no escaping in a Markdown link are kept.
pub(crate) fn attachment_name(file_name: &str) -> (String, String) {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or_default();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.chars().all(char::is_alphanumeric) => {
            (stem, format!(".{ext}"))
        }
        _ => (name, String::new()),
    };
    let stem = clean_name(stem, |c| {
        if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
            c
        } else {
            '-'
        }
    });
    let stem = if stem.is_empty() {
        "file".to_owned()
    } else {
        stem
    };
    (stem, ext)
}

/// Maps every character, trims, forbids a leading dot and cuts to the length limit.
fn clean_name(name: &str, map: impl Fn(char) -> char) -> String {
    let mapped: String = name.chars().map(map).collect();
    let mut clean = mapped.trim().to_owned();
    if clean.starts_with('.') {
        clean.replace_range(..1, "_");
    }
    let mut end = clean.len().min(MAX_STEM_BYTES);
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean.truncate(end);
    clean.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stems_follow_the_title() {
        let cases = [
            ("Einkauf", "Einkauf"),
            ("  padded  ", "padded"),
            ("a/b\\c", "a-b-c"),
            ("tab\there\u{7}", "tab here"),
            (".hidden", "_hidden"),
            ("  .padded and hidden", "_padded and hidden"),
            ("...", "_"),
            ("Fragen? Antworten: \"ja\" <1> *", "Fragen Antworten- ja 1"),
            ("Ende mit Punkt.", "Ende mit Punkt"),
            ("zu   viele\tLeerzeichen", "zu viele Leerzeichen"),
            ("", "Untitled"),
            ("   ", "Untitled"),
        ];
        for (title, stem) in cases {
            assert_eq!(file_stem_for(title), stem, "{title:?}");
        }
    }

    #[test]
    fn long_titles_are_cut_at_a_character_boundary() {
        let stem = file_stem_for(&"ä".repeat(100));
        assert_eq!(stem, "ä".repeat(60));
        let stem = file_stem_for(&format!("{}ä", "a".repeat(119)));
        assert_eq!(stem, "a".repeat(119));
        let stem = file_stem_for(&format!("{} tail", "a".repeat(119)));
        assert_eq!(stem, "a".repeat(119));
    }

    #[test]
    fn attachment_names_need_no_escaping() {
        let cases = [
            ("photo.png", "photo", ".png"),
            (
                "/home/me/My Report (final).PDF",
                "My-Report--final-",
                ".PDF",
            ),
            ("archive.tar.gz", "archive.tar", ".gz"),
            ("Übersicht 2026.pdf", "Übersicht-2026", ".pdf"),
            (".env", "_env", ""),
            ("noext", "noext", ""),
            ("", "file", ""),
            ("weird.ex t", "weird.ex-t", ""),
        ];
        for (name, stem, ext) in cases {
            assert_eq!(
                attachment_name(name),
                (stem.to_owned(), ext.to_owned()),
                "{name:?}"
            );
        }
    }

    #[test]
    fn new_files_get_a_number_instead_of_replacing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            write_new(dir.path(), "Note", " ", ".md", b"1").unwrap(),
            "Note.md"
        );
        assert_eq!(
            write_new(dir.path(), "Note", " ", ".md", b"2").unwrap(),
            "Note 2.md"
        );
        assert_eq!(
            write_new(dir.path(), "Note", "-", ".md", b"3").unwrap(),
            "Note-2.md"
        );
        assert_eq!(fs::read(dir.path().join("Note.md")).unwrap(), b"1");
        assert_eq!(fs::read(dir.path().join("Note 2.md")).unwrap(), b"2");

        let second = dir.path().join("Note 2.md");
        assert_eq!(
            move_new(&second, dir.path(), "Note", " ", ".md").unwrap(),
            "Note 2.md"
        );
        assert_eq!(
            move_new(&second, dir.path(), "Other", " ", ".md").unwrap(),
            "Other.md"
        );
        assert!(!second.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn atomic_write_replaces_content_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

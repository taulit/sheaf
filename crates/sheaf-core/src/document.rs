//! A single Markdown file anywhere on disk, opened for editing as it is: not a note,
//! so it gets no id, is not indexed and is never renamed or moved.
//!
//! Only the body is edited. The byte order mark and a frontmatter block in front of it
//! are written back unchanged. Every write replaces the file atomically through a
//! temporary file beside it and keeps its permissions; a symbolic link is followed,
//! so the file it points to is the one written.
//!
//! Changes by other programs are told apart from the reader's own by the file's
//! [`Fingerprint`]: [`save`] refuses to overwrite a file that changed since it was
//! read, unless told to.

use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::frontmatter;
use crate::i18n::tr;
use crate::store::fsops::write_atomic;
use crate::store::{Fingerprint, timestamp};

/// A Markdown file as it was read or last written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    /// The file, with symbolic links resolved.
    pub path: PathBuf,
    /// The file exactly as stored.
    pub text: String,
    pub fingerprint: Fingerprint,
    body_start: usize,
}

/// What came of [`save`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saved {
    /// The body is on disk; the document as it is now.
    Written(Document),
    /// Another program changed the file; nothing was written. The file as it is now.
    Changed(Document),
    /// The file is gone; nothing was written.
    Deleted,
}

/// What [`check`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checked {
    Same,
    /// Another program changed the file; the file as it is now.
    Changed(Document),
    Deleted,
}

impl Document {
    /// Reads the file at `path`. Fails for a missing file, for anything that is not a
    /// regular file and for a file that is not UTF-8 text.
    pub fn load(path: &Path) -> Result<Document> {
        let not_found = |err: io::Error| match err.kind() {
            io::ErrorKind::NotFound => Error::NotFound(path.display().to_string()),
            _ => Error::Io(err),
        };
        let path = fs::canonicalize(path).map_err(not_found)?;
        // Stat before read: if the file changes in between, the fingerprint is the older
        // one and the next guarded write reports the change.
        let mut file = File::open(&path).map_err(not_found)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::Invalid(tr("not a file")));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        file.read_to_end(&mut bytes)?;
        let text = String::from_utf8(bytes).map_err(|_| Error::Invalid(tr("not a text file")))?;
        let body_start = text.len() - frontmatter::split(&text).body.len();
        Ok(Document {
            path,
            text,
            fingerprint: fingerprint_of(&metadata),
            body_start,
        })
    }

    /// The Markdown text without byte order mark and frontmatter.
    pub fn body(&self) -> &str {
        &self.text[self.body_start..]
    }

    /// The byte order mark and frontmatter block in front of the body.
    pub fn head(&self) -> &str {
        &self.text[..self.body_start]
    }

    /// When the file was last changed.
    pub fn modified(&self) -> Timestamp {
        self.fingerprint.modified
    }

    /// The file name, for a title.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// `body` behind the head of this document.
    fn with_body(&self, body: &str) -> String {
        frontmatter::join(self.head(), body)
    }
}

/// Writes `body` into the file of `known`, behind its head. Unless `force` is set,
/// nothing is written if the file changed or went away since `known` was read; with
/// `force` the file is overwritten or written anew.
pub fn save(known: &Document, body: &str, force: bool) -> Result<Saved> {
    if !force {
        match check(known)? {
            Checked::Same => {}
            Checked::Changed(now) => return Ok(Saved::Changed(now)),
            Checked::Deleted => return Ok(Saved::Deleted),
        }
    }
    let text = known.with_body(body);
    write_atomic(&known.path, text.as_bytes())?;
    let metadata = fs::metadata(&known.path)?;
    Ok(Saved::Written(Document {
        path: known.path.clone(),
        body_start: text.len() - body.len(),
        text,
        fingerprint: fingerprint_of(&metadata),
    }))
}

/// Whether the file of `known` is still as it was read.
pub fn check(known: &Document) -> Result<Checked> {
    let metadata = match fs::metadata(&known.path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Checked::Deleted),
        Err(err) => return Err(err.into()),
    };
    if fingerprint_of(&metadata) == known.fingerprint {
        return Ok(Checked::Same);
    }
    match Document::load(&known.path) {
        Ok(now) => Ok(Checked::Changed(now)),
        Err(Error::NotFound(_)) => Ok(Checked::Deleted),
        Err(err) => Err(err),
    }
}

fn fingerprint_of(metadata: &fs::Metadata) -> Fingerprint {
    Fingerprint {
        modified: timestamp(metadata.modified()).unwrap_or_default(),
        size: metadata.len(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn written(saved: Saved) -> Document {
        match saved {
            Saved::Written(document) => document,
            other => panic!("not written: {other:?}"),
        }
    }

    #[test]
    fn the_head_survives_and_only_the_body_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("README.md");
        let text = "\u{feff}---\r\ntitle: Projekt # comment\r\nfoo: [a, b]\r\n---\r\n# Projekt\r\n";
        fs::write(&path, text).unwrap();
        let document = Document::load(&path).unwrap();
        assert_eq!(document.body(), "# Projekt\r\n");
        assert_eq!(document.file_name(), "README.md");

        let saved = written(save(&document, "# Projekt\n\nNeu.\n", false).unwrap());
        let expected =
            "\u{feff}---\r\ntitle: Projekt # comment\r\nfoo: [a, b]\r\n---\r\n# Projekt\n\nNeu.\n";
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(saved.text, expected);
        assert_eq!(saved.body(), "# Projekt\n\nNeu.\n");
        assert_eq!(check(&saved).unwrap(), Checked::Same);
    }

    #[test]
    fn a_change_elsewhere_is_not_overwritten_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.md");
        fs::write(&path, "eins\n").unwrap();
        let document = Document::load(&path).unwrap();
        fs::write(&path, "zwei, von woanders\n").unwrap();

        let Saved::Changed(now) = save(&document, "mein Text\n", false).unwrap() else {
            panic!("the change was not noticed");
        };
        assert_eq!(now.body(), "zwei, von woanders\n");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "zwei, von woanders\n",
            "nothing written"
        );
        assert!(matches!(check(&document).unwrap(), Checked::Changed(_)));

        written(save(&document, "mein Text\n", true).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "mein Text\n");
    }

    #[test]
    fn a_deleted_file_is_written_again_only_when_forced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weg.md");
        fs::write(&path, "text\n").unwrap();
        let document = Document::load(&path).unwrap();
        fs::remove_file(&path).unwrap();

        assert_eq!(check(&document).unwrap(), Checked::Deleted);
        assert_eq!(save(&document, "neu\n", false).unwrap(), Saved::Deleted);
        assert!(!path.exists());
        written(save(&document, "neu\n", true).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "neu\n");
    }

    #[test]
    fn links_are_followed_and_permissions_kept() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("echt.md");
        fs::write(&target, "a\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let document = Document::load(&link).unwrap();
        assert_eq!(document.path, fs::canonicalize(&target).unwrap());
        written(save(&document, "b\n", false).unwrap());
        assert!(
            link.symlink_metadata().unwrap().is_symlink(),
            "still a link"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "b\n");
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn only_text_files_open() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            Document::load(&dir.path().join("fehlt.md")),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(Document::load(dir.path()), Err(Error::Invalid(_))));
        let binary = dir.path().join("bild.md");
        fs::write(&binary, [0xff, 0xfe, 0x00, 0x9f]).unwrap();
        assert!(matches!(Document::load(&binary), Err(Error::Invalid(_))));
    }
}

//! The types exchanged between storage, search, the D-Bus service, the CLI and the app.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Identifier of a note.
///
/// A note written by Sheaf carries a ULID in its frontmatter (`id:`), which stays the
/// same when the note is renamed or moved. A file that Sheaf has never written (created
/// by another editor, synced in, imported) has no frontmatter to hold one, and Sheaf
/// does not touch files just to stamp them. Such a note gets a *provisional* id derived
/// from its path relative to the notes folder. It is replaced by a real ULID the first
/// time Sheaf writes the note.
///
/// The two forms cannot collide: ULIDs are 26 upper-case Crockford base32 characters,
/// provisional ids are `p` followed by 25 lower-case ones.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(String);

const CROCKFORD_LOWER: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

impl NoteId {
    /// A fresh ULID for a note Sheaf is about to write.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        NoteId(ulid::Ulid::generate().to_string())
    }

    /// The provisional id of a file without an `id:` in its frontmatter.
    ///
    /// `rel_path` is the path relative to the notes folder with `/` separators.
    pub fn provisional(rel_path: &str) -> Self {
        // FNV-1a, 128 bit: stable across Rust versions and platforms, unlike `DefaultHasher`.
        let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
        for byte in rel_path.as_bytes() {
            hash ^= u128::from(*byte);
            hash = hash.wrapping_mul(0x0000000001000000000000000000013b);
        }
        let mut id = String::with_capacity(26);
        id.push('p');
        for i in (0..25).rev() {
            let index = ((hash >> (i * 5)) & 0x1f) as usize;
            id.push(CROCKFORD_LOWER[index] as char);
        }
        NoteId(id)
    }

    pub fn is_provisional(&self) -> bool {
        self.0.starts_with('p')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `text` has the shape of an id (either form). Used to tell ids from titles.
    pub fn looks_like_id(text: &str) -> bool {
        text.parse::<NoteId>().is_ok()
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for NoteId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let provisional = text.len() == 26
            && text.starts_with('p')
            && text[1..].bytes().all(|b| CROCKFORD_LOWER.contains(&b));
        if provisional || ulid::Ulid::from_string(text).is_ok_and(|_| text.len() == 26) {
            // ULIDs are stored upper-case; accept what the user typed.
            let id = if provisional {
                text.to_owned()
            } else {
                text.to_ascii_uppercase()
            };
            Ok(NoteId(id))
        } else {
            Err(Error::Invalid(format!("not a note id: {text}")))
        }
    }
}

/// What the gallery, lists, the CLI and D-Bus results need to know about a note without
/// reading its text. The index stores all of it, so listing never opens note files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteMeta {
    pub id: NoteId,
    /// Path relative to the notes folder, `/`-separated, e.g. `Arbeit/Ausschreibung.md`.
    pub rel_path: String,
    /// Folder relative to the notes folder, `/`-separated; empty for the root.
    pub folder: String,
    /// First line of the note without Markdown heading markers.
    pub title: String,
    /// Frontmatter `created:`; for files without it the file's birth time, else its mtime.
    pub created: Timestamp,
    /// The file's modification time.
    pub modified: Timestamp,
    pub pinned: bool,
    /// Tags from `#tag` in the text and `tags:` in the frontmatter, without `#`, in the
    /// spelling of their first occurrence, deduplicated case-insensitively.
    pub tags: Vec<String>,
    pub tasks_open: u32,
    pub tasks_total: u32,
    pub has_image: bool,
    pub has_table: bool,
    /// Links to files other than images (`_attachments/…` or any relative non-note file).
    pub has_attachment: bool,
    /// The file name marks it as a sync conflict copy (Syncthing, Nextcloud, Dropbox).
    pub conflict: bool,
}

impl NoteMeta {
    pub fn has_checklist(&self) -> bool {
        self.tasks_total > 0
    }
}

/// A note as the index consumes it: [`NoteMeta`] plus the searchable text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteDoc {
    pub meta: NoteMeta,
    /// Text of all headings after the title line, without `#` markers.
    pub headings: Vec<String>,
    /// The Markdown text without frontmatter, exactly as stored.
    pub body: String,
    /// Targets of `[[wiki links]]` (the part before `|` or `#`), for backlinks.
    pub links: Vec<String>,
}

/// The smallest handle on a note: enough to name it in messages and candidate lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRef {
    pub id: NoteId,
    pub title: String,
    pub folder: String,
}

impl From<&NoteMeta> for NoteRef {
    fn from(meta: &NoteMeta) -> Self {
        NoteRef {
            id: meta.id.clone(),
            title: meta.title.clone(),
            folder: meta.folder.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_ulids_and_round_trip() {
        let id = NoteId::new();
        assert_eq!(id.as_str().len(), 26);
        assert!(!id.is_provisional());
        assert_eq!(id.as_str().parse::<NoteId>().unwrap(), id);
        assert_eq!(id.as_str().to_lowercase().parse::<NoteId>().unwrap(), id);
    }

    #[test]
    fn provisional_ids_are_stable_and_distinct() {
        let a = NoteId::provisional("Arbeit/Ausschreibung.md");
        assert_eq!(a, NoteId::provisional("Arbeit/Ausschreibung.md"));
        assert_ne!(a, NoteId::provisional("Arbeit/Ausschreibung 2.md"));
        assert!(a.is_provisional());
        assert_eq!(a.as_str().len(), 26);
        assert_eq!(a.as_str().parse::<NoteId>().unwrap(), a);
        // Pinned so that ids survive refactoring of the hash.
        assert_eq!(
            NoteId::provisional("a.md").as_str(),
            "p9e2sp7zkne9vvg1q9ev2b7zxq"
        );
    }

    #[test]
    fn titles_are_not_ids() {
        assert!(!NoteId::looks_like_id("Einkauf"));
        assert!(!NoteId::looks_like_id("p"));
        assert!(!NoteId::looks_like_id("Protokoll vom 12. November"));
        assert!(NoteId::looks_like_id("01JD3K8Z6Q4X7V9B2M5N8P0R3T"));
    }
}

//! The error type shared by storage and search. Its messages are for people, in their
//! language; [`Error::code`] is for programs.

use std::fmt;

use crate::i18n::tr;
use crate::model::NoteRef;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum Error {
    /// No note, folder or template matches the reference.
    NotFound(String),

    /// A title or title prefix matches more than one note.
    Ambiguous {
        query: String,
        candidates: Vec<NoteRef>,
    },

    /// The caller passed something that can never work, such as a folder outside the root.
    Invalid(String),

    Exists(String),

    /// The file changed on disk since the caller last read it; nothing was written.
    Conflict(String),

    Config(String),

    Index(String),

    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (message, what) = match self {
            Error::NotFound(what) => (tr("not found: {}"), what),
            Error::Ambiguous { query, .. } => (tr("ambiguous reference: {}"), query),
            Error::Invalid(what) => (tr("invalid input: {}"), what),
            Error::Exists(what) => (tr("already exists: {}"), what),
            Error::Conflict(what) => (tr("changed on disk: {}"), what),
            Error::Config(what) => (tr("configuration: {}"), what),
            Error::Index(what) => (tr("search index: {}"), what),
            Error::Io(err) => return err.fmt(f),
        };
        f.write_str(&message.replace("{}", what))
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => err.source(),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl Error {
    /// Stable machine-readable code, used in CLI JSON errors and D-Bus error names.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound(_) => "not_found",
            Error::Ambiguous { .. } => "ambiguous",
            Error::Invalid(_) => "invalid",
            Error::Exists(_) => "exists",
            Error::Conflict(_) => "conflict",
            Error::Config(_) => "config",
            Error::Index(_) => "index",
            Error::Io(_) => "io",
        }
    }
}

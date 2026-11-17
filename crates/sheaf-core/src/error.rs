//! The error type shared by storage and search.

use crate::model::NoteRef;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No note, folder or template matches the reference.
    #[error("not found: {0}")]
    NotFound(String),

    /// A title or title prefix matches more than one note.
    #[error("ambiguous reference: {query}")]
    Ambiguous {
        query: String,
        candidates: Vec<NoteRef>,
    },

    /// The caller passed something that can never work, such as a folder outside the root.
    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("already exists: {0}")]
    Exists(String),

    /// The file changed on disk since the caller last read it; nothing was written.
    #[error("changed on disk: {0}")]
    Conflict(String),

    #[error("configuration: {0}")]
    Config(String),

    #[error("search index: {0}")]
    Index(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
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

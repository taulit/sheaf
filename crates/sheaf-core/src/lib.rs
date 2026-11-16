//! Core of the Sheaf notes app: plain Markdown files on disk, a tantivy index over
//! them, and the search language shared by the app, the CLI and the D-Bus service.
//!
//! Nothing in this crate opens a window or talks to D-Bus. The layering is:
//!
//! - [`model`]: the types every other part exchanges.
//! - storage (`paths`, `frontmatter`, `scan`, `store`, `trash`, `config`, `templates`,
//!   `watch`): reads and writes the notes folder.
//! - search (`analysis`, `query`, `index`, `snippet`): indexes [`model::NoteDoc`]s and
//!   answers queries.

pub mod config;
pub mod error;
pub mod frontmatter;
pub mod i18n;
pub mod model;
pub mod paths;
pub mod scan;
pub mod store;
pub mod templates;
pub mod trash;

pub use config::{AppConfig, FolderConfig};
pub use error::{Error, Result};
pub use model::{NoteDoc, NoteId, NoteMeta, NoteRef};
pub use store::{CreateOptions, Entry, Fingerprint, Note, NoteStore, TaskRef, WriteOptions};
pub use trash::{TRASH_MAX_AGE, TrashedNote};

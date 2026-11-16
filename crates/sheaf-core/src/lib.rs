//! Core of the Sheaf notes app: plain Markdown files on disk, a tantivy index over
//! them, and the search language shared by the app, the CLI and the D-Bus service.
//!
//! Nothing in this crate opens a window or talks to D-Bus. The layering is:
//!
//! - [`model`]: the types every other part exchanges.
//! - storage (`frontmatter`, `scan`, `store`, `trash`, `config`, `templates`, `watch`):
//!   reads and writes the notes folder.
//! - search (`analysis`, `query`, `index`, `snippet`): indexes [`model::NoteDoc`]s and
//!   answers queries.

pub mod error;
pub mod model;

pub use error::{Error, Result};
pub use model::{NoteDoc, NoteId, NoteMeta, NoteRef};

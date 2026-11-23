//! Core of the Sheaf notes app: plain Markdown files on disk, a tantivy index over
//! them, and the search language shared by the app, the CLI and the D-Bus service.
//!
//! Nothing in this crate opens a window or talks to D-Bus. The layering is:
//!
//! - [`model`]: the types every other part exchanges.
//! - storage ([`paths`], [`frontmatter`], [`scan`], [`store`], [`trash`], [`config`],
//!   [`templates`], [`watch`]): reads and writes the notes folder; [`document`] reads and
//!   writes a single Markdown file outside it.
//! - search ([`analysis`], [`query`], [`index`], [`search`], [`snippet`]): indexes
//!   [`model::NoteDoc`]s and answers queries.

pub mod analysis;
pub mod config;
pub mod document;
pub mod error;
pub mod frontmatter;
pub mod i18n;
pub mod index;
pub mod logger;
pub mod model;
pub mod paths;
pub mod query;
pub mod scan;
pub mod search;
pub mod snippet;
pub mod store;
pub mod templates;
#[cfg(feature = "testgen")]
pub mod testgen;
pub mod trash;
pub mod watch;

pub use config::{AppConfig, FolderConfig};
pub use error::{Error, Result};
pub use index::{Reader, SearchIndex, Sort, Writer};
pub use model::{NoteDoc, NoteId, NoteMeta, NoteRef};
pub use query::{Feature, Filter, ItemKind, ParsedQuery, Period, QueryItem};
pub use search::{Hit, SearchResults};
pub use snippet::Snippet;
pub use store::{CreateOptions, Entry, Fingerprint, Note, NoteStore, TaskRef, WriteOptions};
pub use trash::{TRASH_MAX_AGE, TrashedNote};
pub use watch::{Change, WatchHandle, watch};

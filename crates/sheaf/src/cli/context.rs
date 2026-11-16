//! What every command needs: the notes folder, the output mode and the printer.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::Serialize;
use sheaf_core::paths::Env;
use sheaf_core::{NoteMeta, NoteStore};

use super::error::Result;
use super::output::{Envelope, NoteJson, NoteResult};

/// The version of the JSON documents. Additive changes keep it, breaking ones raise it.
pub const SCHEMA: u32 = 1;

#[derive(Debug)]
pub struct Ctx {
    pub env: Env,
    pub notes_dir: PathBuf,
    pub store: NoteStore,
    pub json: bool,
    pub color: bool,
    zone: TimeZone,
}

impl Ctx {
    pub fn new(dir: Option<&Path>, json: bool) -> Result<Self> {
        let env = Env::from_process();
        let notes_dir = env.notes_dir(dir)?;
        let notes_dir = std::path::absolute(&notes_dir).unwrap_or(notes_dir);
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Ok(Ctx {
            store: NoteStore::new(&notes_dir),
            env,
            notes_dir,
            json,
            color: !no_color && io::stdout().is_terminal(),
            zone: TimeZone::system(),
        })
    }

    pub fn note_json<'a>(&self, meta: &'a NoteMeta) -> NoteJson<'a> {
        NoteJson::new(meta, &self.notes_dir)
    }

    /// Prints `body` as the JSON document, or the text `human` builds.
    pub fn emit<T: Serialize>(&self, body: &T, human: impl FnOnce(&Self) -> String) -> Result {
        let mut out = if self.json {
            json_document(body)?
        } else {
            human(self)
        };
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        write_stdout(&out)?;
        Ok(())
    }

    /// The answer of every command that changes one note: the note, or its id.
    pub fn emit_note(&self, meta: &NoteMeta) -> Result {
        let note = self.note_json(meta);
        self.emit(&NoteResult { note }, |_| meta.id.to_string())
    }

    /// A point in time for people: local time to the minute.
    pub fn local(&self, time: Timestamp) -> String {
        time.to_zoned_datetime(self.zone.clone())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }
}

/// The one JSON document a command prints: `body` behind the schema version.
pub fn json_document<T: Serialize>(body: &T) -> io::Result<String> {
    serde_json::to_string(&Envelope {
        schema: SCHEMA,
        body,
    })
    .map_err(io::Error::other)
}

/// Writes to stdout in one go; a reader that went away (`| head`) is not an error.
pub fn write_stdout(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

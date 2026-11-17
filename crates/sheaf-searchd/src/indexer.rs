//! Writing the index, in a child process.
//!
//! Tantivy's smallest writer allocates and zeroes about 8 MB, and glibc keeps that memory
//! (often in a fresh arena per writer thread) after the writer is gone. The service
//! therefore never writes the index itself: it runs `sheaf-searchd --index NOTES INDEX`,
//! which reads the changes from stdin, commits them, prints the ids of the indexed notes
//! and exits, taking its memory with it.

use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use sheaf_core::{Error, NoteStore, Result, SearchIndex};

/// The hidden flag that runs [`serve`].
pub const FLAG: &str = "--index";

/// One change to the index.
#[derive(Debug)]
pub enum Op {
    /// Index the note file at this relative path, or drop it if it cannot be read.
    Upsert(String),
    /// Drop the note indexed under this relative path.
    Remove(String),
    /// Drop every note.
    Clear,
}

/// Applies `ops` in a child process and returns the ids of the notes it indexed.
pub fn apply(notes_dir: &Path, index_dir: &Path, ops: &[Op]) -> Result<Vec<String>> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg(FLAG)
        .args([notes_dir, index_dir])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = Vec::new();
    for op in ops {
        match op {
            Op::Upsert(path) => input.extend_from_slice(format!("U{path}\0").as_bytes()),
            Op::Remove(path) => input.extend_from_slice(format!("R{path}\0").as_bytes()),
            Op::Clear => input.extend_from_slice(b"C\0"),
        }
    }
    let written = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no stdin"))?
        .write_all(&input);
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_string(&mut output)?;
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(Error::Index(format!("the indexer failed ({status})")));
    }
    written?;
    Ok(output.lines().map(str::to_owned).collect())
}

/// The child's side of [`apply`]: reads the changes from stdin, commits them and prints
/// one id per indexed note.
pub fn serve(notes_dir: &Path, index_dir: &Path) -> Result<()> {
    let mut ops = Vec::new();
    for item in io::stdin().lock().split(0) {
        let item = String::from_utf8(item?)
            .map_err(|_| Error::Invalid("a path that is not UTF-8".to_owned()))?;
        let op = match item.split_at_checked(1) {
            Some(("U", path)) => Op::Upsert(path.to_owned()),
            Some(("R", path)) => Op::Remove(path.to_owned()),
            Some(("C", "")) => Op::Clear,
            _ => return Err(Error::Invalid(format!("unknown index change: {item}"))),
        };
        ops.push(op);
    }
    let ids = run(notes_dir, index_dir, ops)?;
    let mut stdout = io::stdout().lock();
    for id in ids {
        writeln!(stdout, "{id}")?;
    }
    Ok(())
}

/// Applies `ops` in this process and returns the ids of the notes it indexed.
pub fn run(
    notes_dir: &Path,
    index_dir: &Path,
    ops: impl IntoIterator<Item = Op>,
) -> Result<Vec<String>> {
    let store = NoteStore::new(notes_dir);
    let index = SearchIndex::open_or_create(index_dir)?;
    let mut writer = index.writer()?;
    let mut ids = Vec::new();
    for op in ops {
        match op {
            Op::Upsert(path) => match store.load_doc(&path) {
                Ok(doc) => {
                    writer.upsert(&doc)?;
                    ids.push(doc.meta.id.to_string());
                }
                Err(err) => {
                    log::warn!("not indexing {path}: {err}");
                    writer.remove_path(&path);
                }
            },
            Op::Remove(path) => writer.remove_path(&path),
            Op::Clear => writer.clear()?,
        }
    }
    writer.finish()?;
    Ok(ids)
}

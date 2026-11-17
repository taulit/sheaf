//! The JSON documents of the commands and the text layout shared by the human output.

use std::ops::Range;
use std::path::Path;

use jiff::Timestamp;
use serde::Serialize;
use sheaf_core::{Hit, NoteId, NoteMeta, TrashedNote, i18n::tr};

/// Every document starts with the schema version.
#[derive(Serialize)]
pub struct Envelope<'a, T: Serialize> {
    pub schema: u32,
    #[serde(flatten)]
    pub body: &'a T,
}

/// A note, the same in every document. `path` is the absolute path of the file.
#[derive(Serialize)]
pub struct NoteJson<'a> {
    id: &'a NoteId,
    title: &'a str,
    folder: &'a str,
    path: String,
    created: Timestamp,
    modified: Timestamp,
    pinned: bool,
    tags: &'a [String],
    tasks_open: u32,
    tasks_total: u32,
    has_image: bool,
    has_table: bool,
    has_attachment: bool,
    conflict: bool,
}

impl<'a> NoteJson<'a> {
    pub fn new(meta: &'a NoteMeta, root: &Path) -> Self {
        NoteJson {
            id: &meta.id,
            title: &meta.title,
            folder: &meta.folder,
            path: root.join(&meta.rel_path).to_string_lossy().into_owned(),
            created: meta.created,
            modified: meta.modified,
            pinned: meta.pinned,
            tags: &meta.tags,
            tasks_open: meta.tasks_open,
            tasks_total: meta.tasks_total,
            has_image: meta.has_image,
            has_table: meta.has_table,
            has_attachment: meta.has_attachment,
            conflict: meta.conflict,
        }
    }
}

#[derive(Serialize)]
pub struct NoteResult<'a> {
    pub note: NoteJson<'a>,
}

#[derive(Serialize)]
pub struct ShowResult<'a> {
    pub note: NoteJson<'a>,
    pub body: &'a str,
}

#[derive(Serialize)]
pub struct NoteList<'a> {
    pub notes: Vec<NoteJson<'a>>,
}

#[derive(Serialize)]
pub struct HitJson<'a> {
    #[serde(flatten)]
    note: NoteJson<'a>,
    score: f32,
    snippet: &'a str,
    snippet_highlights: Vec<[usize; 2]>,
    title_highlights: Vec<[usize; 2]>,
}

impl<'a> HitJson<'a> {
    pub fn new(hit: &'a Hit, root: &Path) -> Self {
        HitJson {
            note: NoteJson::new(&hit.meta, root),
            score: hit.score,
            snippet: &hit.snippet.text,
            snippet_highlights: pairs(&hit.snippet.highlights),
            title_highlights: pairs(&hit.title_highlights),
        }
    }
}

fn pairs(ranges: &[Range<usize>]) -> Vec<[usize; 2]> {
    ranges
        .iter()
        .map(|range| [range.start, range.end])
        .collect()
}

#[derive(Serialize)]
pub struct SearchResult<'a> {
    pub query: &'a str,
    pub total: usize,
    pub hits: Vec<HitJson<'a>>,
}

#[derive(Serialize)]
pub struct TaskJson {
    pub note_id: NoteId,
    pub note_title: String,
    pub number: u32,
    pub checked: bool,
    pub text: String,
}

#[derive(Serialize)]
pub struct TaskList {
    pub tasks: Vec<TaskJson>,
}

#[derive(Serialize)]
pub struct TagJson {
    pub name: String,
    pub count: usize,
}

#[derive(Serialize)]
pub struct TagList {
    pub tags: Vec<TagJson>,
}

#[derive(Serialize)]
pub struct FolderJson {
    pub path: String,
    pub notes: usize,
}

#[derive(Serialize)]
pub struct FolderList {
    pub folders: Vec<FolderJson>,
}

#[derive(Serialize)]
pub struct TrashedJson<'a> {
    #[serde(flatten)]
    note: NoteJson<'a>,
    deleted: Timestamp,
    from: &'a str,
}

impl<'a> TrashedJson<'a> {
    pub fn new(trashed: &'a TrashedNote, root: &Path) -> Self {
        TrashedJson {
            note: NoteJson::new(&trashed.meta, root),
            deleted: trashed.deleted,
            from: &trashed.from,
        }
    }
}

#[derive(Serialize)]
pub struct TrashList<'a> {
    pub notes: Vec<TrashedJson<'a>>,
}

#[derive(Serialize)]
pub struct PathResult {
    pub path: String,
}

#[derive(Serialize)]
pub struct Done {
    pub ok: bool,
}

/// The title as people read it: never empty, never longer than a terminal line.
pub fn display_title(title: &str) -> String {
    const MAX_CHARS: usize = 60;
    if title.is_empty() {
        return tr("Untitled");
    }
    match title.char_indices().nth(MAX_CHARS) {
        Some((end, _)) => format!("{}…", title[..end].trim_end()),
        None => title.to_owned(),
    }
}

pub fn display_folder(folder: &str) -> &str {
    if folder.is_empty() { "/" } else { folder }
}

/// Left-aligns the cells of each row in columns two spaces apart; the last column is
/// not padded.
pub fn table(rows: &[Vec<String>]) -> Vec<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            let cells = rows.iter().filter_map(|row| row.get(column));
            cells.map(|cell| cell.chars().count()).max().unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let last = row.len().saturating_sub(1);
            let mut line = String::new();
            for (column, cell) in row.iter().enumerate() {
                line.push_str(cell);
                if column < last {
                    let padding = widths[column] - cell.chars().count() + 2;
                    line.extend(std::iter::repeat_n(' ', padding));
                }
            }
            line
        })
        .collect()
}

/// `text` with the highlighted byte ranges in bold, when `color` is on.
pub fn highlighted(text: &str, ranges: &[Range<usize>], color: bool) -> String {
    if !color {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + ranges.len() * 8);
    let mut at = 0;
    for range in ranges {
        if range.start < at
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        out.push_str(&text[at..range.start]);
        out.push_str("\x1b[1m");
        out.push_str(&text[range.clone()]);
        out.push_str("\x1b[22m");
        at = range.end;
    }
    out.push_str(&text[at..]);
    out
}

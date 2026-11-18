use std::collections::HashMap;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use sheaf_core::i18n::tr;
use sheaf_core::index::Sort;
use sheaf_core::{Filter, NoteMeta, Reader, SearchIndex, query, snippet};

use super::{now, resolve};
use crate::cli::args::{Format, ListArgs, SearchArgs, ShowArgs, SortKey, TasksArgs};
use crate::cli::context::{Ctx, write_stdout};
use crate::cli::dbus;
use crate::cli::error::{CliError, Result};
use crate::cli::output::{
    FolderJson, FolderList, HitJson, NoteList, PathResult, SearchResult, ShowResult, TagJson,
    TagList, TaskJson, TaskList, TrashList, TrashedJson, display_folder, display_title,
    highlighted, table,
};

/// A reader for `search`, `list`, `tags` and `folders`: the on-disk index when the
/// service for this folder is up to date, else an index built in memory from the files.
fn reader(ctx: &Ctx) -> Result<Reader> {
    if dbus::ask_service(&ctx.notes_dir, dbus::Ask::Sync)
        && let Ok(Some(index)) = ctx
            .env
            .index_dir(&ctx.notes_dir)
            .map_err(CliError::from)
            .and_then(|dir| SearchIndex::open_read_only(dir).map_err(CliError::from))
    {
        let reader = index.reader();
        reader.reload()?;
        return Ok(reader);
    }
    let index = SearchIndex::in_memory()?;
    let mut writer = index.writer()?;
    for doc in ctx.store.all_docs()? {
        writer.upsert(&doc)?;
    }
    writer.commit()?;
    Ok(index.reader())
}

pub fn show(ctx: &Ctx, args: ShowArgs) -> Result {
    let meta = resolve(ctx, &args.note.note)?;
    let note = ctx.store.get(&meta.id)?;
    if ctx.json {
        let note_json = ctx.note_json(&note.meta);
        return ctx.emit(
            &ShowResult {
                note: note_json,
                body: note.body(),
            },
            |_| String::new(),
        );
    }
    let text = match args.format {
        // `--format json` turns the JSON mode on before the context is made.
        Format::Md | Format::Json => note.body().to_owned(),
        Format::Text => snippet::plain_text(note.body()) + "\n",
    };
    Ok(write_stdout(&text)?)
}

pub fn search(ctx: &Ctx, args: SearchArgs) -> Result {
    let text = args.query.join(" ");
    let results = reader(ctx)?.search(&query::parse(&text), args.limit, &now())?;
    let result = SearchResult {
        query: &text,
        total: results.total,
        hits: results
            .hits
            .iter()
            .map(|hit| HitJson::new(hit, &ctx.notes_dir))
            .collect(),
    };
    ctx.emit(&result, |ctx| {
        let rows: Vec<Vec<String>> = results
            .hits
            .iter()
            .map(|hit| note_row(ctx, &hit.meta))
            .collect();
        let mut out = String::new();
        for (line, hit) in table(&rows).iter().zip(&results.hits) {
            out.push_str(line);
            out.push('\n');
            if !hit.snippet.text.is_empty() {
                let snippet = highlighted(&hit.snippet.text, &hit.snippet.highlights, ctx.color);
                out.push_str(&format!("    {snippet}\n"));
            }
        }
        out
    })
}

pub fn list(ctx: &Ctx, args: ListArgs) -> Result {
    let mut filters = Vec::new();
    if let Some(folder) = &args.folder {
        filters.push(Filter::Folder(folder.trim_matches('/').to_owned()));
    }
    if let Some(tag) = &args.tag {
        filters.push(Filter::Tag(tag.trim_start_matches('#').to_owned()));
    }
    if args.pinned {
        filters.push(Filter::Pinned);
    }
    let since = args
        .modified_since
        .as_deref()
        .map(parse_since)
        .transpose()?;
    let sort = match args.sort {
        SortKey::Modified => Sort::Modified,
        SortKey::Created => Sort::Created,
        SortKey::Title => Sort::Title,
    };
    // A date cannot be a filter of the index, so the limit applies after it.
    let limit = if since.is_some() { None } else { args.limit };
    let mut metas = reader(ctx)?.list(&filters, sort, limit, &now())?;
    if let Some(since) = since {
        metas.retain(|meta| meta.modified >= since);
        metas.truncate(args.limit.unwrap_or(usize::MAX));
    }
    ctx.emit(
        &NoteList {
            notes: metas.iter().map(|meta| ctx.note_json(meta)).collect(),
        },
        |ctx| {
            let rows: Vec<Vec<String>> = metas.iter().map(|meta| note_row(ctx, meta)).collect();
            table(&rows).join("\n")
        },
    )
}

/// `YYYY-MM-DD` (midnight, local time) or RFC 3339.
fn parse_since(text: &str) -> Result<Timestamp> {
    let invalid = || {
        CliError::Usage(format!(
            "{} {text}",
            tr("not a date, use YYYY-MM-DD or RFC 3339:")
        ))
    };
    if let Ok(time) = text.parse::<Timestamp>() {
        return Ok(time);
    }
    let date: Date = text.parse().map_err(|_| invalid())?;
    let midnight = date
        .to_zoned_datetime(TimeZone::system())
        .map_err(|_| invalid())?;
    Ok(midnight.timestamp())
}

pub fn tasks(ctx: &Ctx, args: TasksArgs) -> Result {
    let paths: Vec<String> = match &args.note {
        Some(reference) => vec![resolve(ctx, reference)?.rel_path],
        None => ctx
            .store
            .walk()?
            .into_iter()
            .map(|entry| entry.rel_path)
            .collect(),
    };
    let mut tasks = Vec::new();
    for path in paths {
        let Ok(note) = ctx.store.load(&path) else {
            continue;
        };
        for task in note
            .scan
            .tasks
            .iter()
            .filter(|task| !args.open || !task.checked)
        {
            tasks.push(TaskJson {
                note_id: note.meta.id.clone(),
                note_title: note.meta.title.clone(),
                number: task.number,
                checked: task.checked,
                text: task.text.clone(),
            });
        }
    }
    let list = TaskList { tasks };
    ctx.emit(&list, |_| {
        let mut out = String::new();
        let mut current = None;
        for task in &list.tasks {
            if current != Some(&task.note_id) {
                current = Some(&task.note_id);
                let title = display_title(&task.note_title);
                out.push_str(&format!("{title}  ({})\n", task.note_id));
            }
            let mark = if task.checked { 'x' } else { ' ' };
            out.push_str(&format!("  [{mark}] {}  {}\n", task.number, task.text));
        }
        out
    })
}

pub fn tags(ctx: &Ctx) -> Result {
    let tags = reader(ctx)?.tags()?;
    let list = TagList {
        tags: tags
            .into_iter()
            .map(|(name, count)| TagJson { name, count })
            .collect(),
    };
    ctx.emit(&list, |_| {
        let width = list
            .tags
            .iter()
            .map(|tag| tag.count.to_string().len())
            .max()
            .unwrap_or(0);
        list.tags
            .iter()
            .map(|tag| format!("{:>width$}  #{}", tag.count, tag.name))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

pub fn folders(ctx: &Ctx) -> Result {
    let counts: HashMap<String, usize> = reader(ctx)?.folders()?.into_iter().collect();
    let mut paths = vec![String::new()];
    paths.extend(ctx.store.folders()?);
    let list = FolderList {
        folders: paths
            .into_iter()
            .map(|path| FolderJson {
                notes: counts.get(&path).copied().unwrap_or(0),
                path,
            })
            .filter(|folder| !folder.path.is_empty() || folder.notes > 0)
            .collect(),
    };
    ctx.emit(&list, |_| {
        let width = list
            .folders
            .iter()
            .map(|folder| folder.notes.to_string().len())
            .max()
            .unwrap_or(0);
        list.folders
            .iter()
            .map(|folder| format!("{:>width$}  {}", folder.notes, display_folder(&folder.path)))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

pub fn trash(ctx: &Ctx) -> Result {
    let trashed = ctx.store.list_trash()?;
    let list = TrashList {
        notes: trashed
            .iter()
            .map(|note| TrashedJson::new(note, &ctx.notes_dir))
            .collect(),
    };
    ctx.emit(&list, |ctx| {
        let rows: Vec<Vec<String>> = trashed
            .iter()
            .map(|note| {
                vec![
                    note.meta.id.to_string(),
                    display_title(&note.meta.title),
                    display_folder(&note.from).to_owned(),
                    ctx.local(note.deleted),
                ]
            })
            .collect();
        table(&rows).join("\n")
    })
}

pub fn path(ctx: &Ctx) -> Result {
    let path = ctx.notes_dir.to_string_lossy().into_owned();
    ctx.emit(&PathResult { path: path.clone() }, |_| path)
}

/// `id  title  folder  modified`
fn note_row(ctx: &Ctx, meta: &NoteMeta) -> Vec<String> {
    vec![
        meta.id.to_string(),
        display_title(&meta.title),
        display_folder(&meta.folder).to_owned(),
        ctx.local(meta.modified),
    ]
}

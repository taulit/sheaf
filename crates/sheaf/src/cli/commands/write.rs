use std::process::Command;

use sheaf_core::i18n::tr;
use sheaf_core::{CreateOptions, Error, NoteId, NoteMeta, NoteRef, TaskRef, templates};

use super::{now, required_text, resolve};
use crate::cli::args::{MvArgs, NewArgs, TaskArgs, TextArgs};
use crate::cli::context::Ctx;
use crate::cli::error::{CliError, Result};

/// A folder as typed: `/` and `.` are the notes folder itself.
fn folder_arg(folder: &str) -> &str {
    match folder.trim().trim_start_matches('/') {
        "." => "",
        folder => folder,
    }
}

pub fn new(ctx: &Ctx, args: NewArgs) -> Result {
    let text = match &args.template {
        Some(name) => {
            let given = super::input_text(args.text.as_deref())?.unwrap_or_default();
            let (title, rest) = given.split_once('\n').unwrap_or((&given, ""));
            let mut text = templates::render(&ctx.notes_dir, name, title.trim(), &now())?;
            if !rest.is_empty() {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(rest);
            }
            text
        }
        None => required_text(args.text.as_deref())?,
    };
    if text.trim().is_empty() {
        return Err(CliError::Usage(tr("the note would be empty")));
    }
    let options = CreateOptions {
        folder: args
            .folder
            .as_deref()
            .map(folder_arg)
            .unwrap_or_default()
            .to_owned(),
        tags: args.tags,
        pinned: args.pin,
        created: None,
    };
    ctx.emit_note(&ctx.store.create(&text, &options)?)
}

pub fn append(ctx: &Ctx, args: TextArgs) -> Result {
    let text = required_text(args.text.as_deref())?;
    let meta = resolve(ctx, &args.note.note)?;
    ctx.emit_note(&ctx.store.append(&meta.id, &text)?)
}

pub fn prepend(ctx: &Ctx, args: TextArgs) -> Result {
    let text = required_text(args.text.as_deref())?;
    let meta = resolve(ctx, &args.note.note)?;
    ctx.emit_note(&ctx.store.prepend(&meta.id, &text)?)
}

pub fn edit(ctx: &Ctx, reference: &str) -> Result {
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| Error::Invalid(tr("neither $VISUAL nor $EDITOR is set")))?;
    let meta = resolve(ctx, reference)?;

    // Through the shell, like git does, so that `EDITOR="code --wait"` works.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("sheaf-editor")
        .arg(ctx.notes_dir.join(&meta.rel_path))
        .status()?;
    if !status.success() {
        return Err(Error::Io(std::io::Error::other(format!(
            "{editor} exited with {status}"
        )))
        .into());
    }

    ctx.store.refresh();
    let edited = ctx.store.load(&meta.rel_path)?.meta;
    ctx.emit_note(&ctx.store.sync_filename(&edited.id)?)
}

pub fn check(ctx: &Ctx, args: TaskArgs, checked: bool) -> Result {
    let task = match args.task.parse::<u32>() {
        Ok(number) if number > 0 => TaskRef::Number(number),
        _ => TaskRef::Text(args.task),
    };
    let meta = resolve(ctx, &args.note.note)?;
    ctx.emit_note(&ctx.store.set_task(&meta.id, &task, checked)?)
}

pub fn mv(ctx: &Ctx, args: MvArgs) -> Result {
    let meta = resolve(ctx, &args.note.note)?;
    ctx.emit_note(&ctx.store.move_to(&meta.id, folder_arg(&args.folder))?)
}

pub fn rm(ctx: &Ctx, reference: &str) -> Result {
    let meta = resolve(ctx, reference)?;
    ctx.emit_note(&ctx.store.trash(&meta.id, jiff::Timestamp::now())?)
}

pub fn restore(ctx: &Ctx, reference: &str) -> Result {
    let id = resolve_trashed(ctx, reference)?;
    ctx.emit_note(&ctx.store.restore(&id)?)
}

pub fn pin(ctx: &Ctx, reference: &str, pinned: bool) -> Result {
    let meta = resolve(ctx, reference)?;
    ctx.emit_note(&ctx.store.set_pinned(&meta.id, pinned)?)
}

/// Like `NoteStore::resolve`, for the notes in the trash.
fn resolve_trashed(ctx: &Ctx, reference: &str) -> Result<NoteId> {
    let reference = reference.trim();
    let trash: Vec<NoteMeta> = ctx
        .store
        .list_trash()?
        .into_iter()
        .map(|t| t.meta)
        .collect();
    if let Ok(id) = reference.parse::<NoteId>()
        && trash.iter().any(|meta| meta.id == id)
    {
        return Ok(id);
    }
    let needle = reference.to_lowercase();
    if !needle.is_empty() {
        for exact in [true, false] {
            let matches: Vec<&NoteMeta> = trash
                .iter()
                .filter(|meta| {
                    let title = meta.title.to_lowercase();
                    if exact {
                        title == needle
                    } else {
                        title.starts_with(&needle)
                    }
                })
                .collect();
            match matches.as_slice() {
                [] => {}
                [meta] => return Ok(meta.id.clone()),
                _ => {
                    return Err(Error::Ambiguous {
                        query: reference.to_owned(),
                        candidates: matches.into_iter().map(NoteRef::from).collect(),
                    }
                    .into());
                }
            }
        }
    }
    Err(Error::NotFound(reference.to_owned()).into())
}

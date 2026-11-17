//! One function per command, grouped by what they touch.

mod app;
mod read;
mod write;

use std::io::{self, IsTerminal, Read};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use sheaf_core::NoteMeta;

use super::args::Command;
use super::context::Ctx;
use super::error::{CliError, Result};

/// Runs a command that works on a notes folder. `Help` and `Completions` do not need
/// one and are handled before a [`Ctx`] exists.
pub fn run(ctx: &Ctx, command: Command) -> Result {
    match command {
        Command::New(args) => write::new(ctx, args),
        Command::Append(args) => write::append(ctx, args),
        Command::Prepend(args) => write::prepend(ctx, args),
        Command::Show(args) => read::show(ctx, args),
        Command::Edit(args) => write::edit(ctx, &args.note),
        Command::Search(args) => read::search(ctx, args),
        Command::List(args) => read::list(ctx, args),
        Command::Tasks(args) => read::tasks(ctx, args),
        Command::Check(args) => write::check(ctx, args, true),
        Command::Uncheck(args) => write::check(ctx, args, false),
        Command::Tags => read::tags(ctx),
        Command::Folders => read::folders(ctx),
        Command::Mv(args) => write::mv(ctx, args),
        Command::Rm(args) => write::rm(ctx, &args.note),
        Command::Restore(args) => write::restore(ctx, &args.note),
        Command::Pin(args) => write::pin(ctx, &args.note, true),
        Command::Unpin(args) => write::pin(ctx, &args.note, false),
        Command::Trash => read::trash(ctx),
        Command::Open(args) => app::open(ctx, &args.note),
        Command::Quick => app::quick(ctx),
        Command::Reindex => app::reindex(ctx),
        Command::Path => read::path(ctx),
        // Answered before a notes folder is opened, see `cli::execute`.
        Command::Completions(_) | Command::Help(_) => Ok(()),
    }
}

fn resolve(ctx: &Ctx, reference: &str) -> Result<NoteMeta> {
    Ok(ctx.store.resolve(reference)?)
}

fn now() -> jiff::ZonedDateTime {
    Timestamp::now().to_zoned_datetime(TimeZone::system())
}

/// The text of a command: the argument, or stdin for `-` and for no argument when stdin
/// is not a terminal. `None` if there is nowhere to read it from; the command never
/// waits for a person to type.
fn input_text(argument: Option<&str>) -> Result<Option<String>> {
    match argument {
        Some(text) if text != "-" => Ok(Some(text.to_owned())),
        None if io::stdin().is_terminal() => Ok(None),
        _ => {
            let mut text = String::new();
            io::stdin().lock().read_to_string(&mut text)?;
            Ok(Some(text))
        }
    }
}

fn required_text(argument: Option<&str>) -> Result<String> {
    input_text(argument)?
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            CliError::Usage(sheaf_core::i18n::tr(
                "no text given: pass TEXT or pipe it on stdin",
            ))
        })
}

//! The command line: parsing, dispatch and error reporting.

mod args;
mod commands;
mod context;
mod dbus;
mod error;
mod help;
mod output;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use sheaf_core::i18n::{self, tr};

use args::{Cli, Command, Format};
use context::{Ctx, json_document};
use error::{CliError, ErrorBody};

/// Runs `sheaf` with `args` (the first is the program name) and returns its exit status.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    i18n::init();
    let args: Vec<OsString> = args.into_iter().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(err) => return report_usage(&err, asked_for_json(&args)),
    };
    let json = cli.json
        || matches!(&cli.command, Some(Command::Show(show)) if matches!(show.format, Format::Json));
    match execute(cli, json) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => report(&err, json),
    }
}

fn execute(cli: Cli, json: bool) -> error::Result {
    match cli.command {
        None => Err(CliError::Usage(tr(
            "the app is not part of this build; run `sheaf help` for the commands",
        ))),
        Some(Command::Help(args)) => help::help(args, json),
        Some(Command::Completions(args)) => help::completions(args.shell),
        Some(command) => commands::run(&Ctx::new(cli.dir.as_deref(), json)?, command),
    }
}

fn report(err: &CliError, json: bool) -> ExitCode {
    if json {
        let document = json_document(&ErrorEnvelope {
            error: ErrorBody::from(err),
        });
        eprintln!("{}", document.unwrap_or_else(|_| err.to_string()));
    } else {
        eprintln!("sheaf: {err}");
        for candidate in err.candidates().unwrap_or_default() {
            let folder = output::display_folder(&candidate.folder);
            let title = output::display_title(&candidate.title);
            eprintln!("  {}  {title}  {folder}", candidate.id);
        }
    }
    err.exit_code()
}

#[derive(serde::Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

/// Help and version go to stdout with status 0; anything else is a usage error.
fn report_usage(err: &clap::Error, json: bool) -> ExitCode {
    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) || !json
    {
        let _ = err.print();
        return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
    }
    let rendered = err.render().to_string();
    let first_paragraph: Vec<&str> = rendered
        .lines()
        .take_while(|line| !line.is_empty())
        .collect();
    let message = first_paragraph
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let message = message.strip_prefix("error: ").unwrap_or(&message);
    report(&CliError::Usage(message.trim().to_owned()), true)
}

/// Whether `--json` was typed, for errors raised before the arguments were understood.
fn asked_for_json(args: &[OsString]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json")
}

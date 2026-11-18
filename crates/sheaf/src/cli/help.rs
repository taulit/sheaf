//! `help`, `help --json` and `completions`. The machine-readable description is read
//! off the clap command tree, so it follows every change of the command line.

use clap::{Arg, ArgAction, Command, CommandFactory};
use clap_complete::Shell as CompleteShell;
use serde::Serialize;
use sheaf_core::i18n::tr;

use super::args::{Cli, HelpArgs, Shell};
use super::context::{json_document, write_stdout};
use super::error::{CliError, EXIT_CODES, Result};

pub fn help(args: HelpArgs, json: bool) -> Result {
    let mut root = Cli::command();
    root.build();
    if json {
        return Ok(write_stdout(&(json_document(&describe(&root))? + "\n"))?);
    }
    let mut command = &root;
    for name in &args.command {
        command = command
            .find_subcommand(name)
            .ok_or_else(|| CliError::Usage(tr("unknown command: {}").replace("{}", name)))?;
    }
    Ok(write_stdout(
        &command.clone().render_long_help().to_string(),
    )?)
}

pub fn completions(shell: Shell) -> Result {
    let shell = match shell {
        Shell::Bash => CompleteShell::Bash,
        Shell::Zsh => CompleteShell::Zsh,
        Shell::Fish => CompleteShell::Fish,
    };
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "sheaf", &mut script);
    Ok(write_stdout(&String::from_utf8_lossy(&script))?)
}

/// The JSON Schema file in `docs/cli-schema/` that describes what a command prints with
/// `--json`; `None` for commands whose output is not JSON.
pub fn output_schema(command: &str) -> Option<&'static str> {
    Some(match command {
        "new" | "append" | "prepend" | "edit" | "check" | "uncheck" | "mv" | "rm" | "restore"
        | "pin" | "unpin" | "open" => "note.schema.json",
        "show" => "show.schema.json",
        "search" => "search.schema.json",
        "list" => "note-list.schema.json",
        "tasks" => "tasks.schema.json",
        "tags" => "tags.schema.json",
        "folders" => "folders.schema.json",
        "trash" => "trash.schema.json",
        "path" => "path.schema.json",
        "quick" | "reindex" => "done.schema.json",
        "help" => "help.schema.json",
        _ => return None,
    })
}

#[derive(Serialize)]
struct Description {
    name: String,
    version: &'static str,
    about: String,
    global_options: Vec<OptionDoc>,
    exit_codes: Vec<ExitCodeDoc>,
    error_schema: &'static str,
    commands: Vec<CommandDoc>,
}

#[derive(Serialize)]
struct ExitCodeDoc {
    code: u8,
    meaning: &'static str,
}

#[derive(Serialize)]
struct CommandDoc {
    name: String,
    about: String,
    usage: String,
    arguments: Vec<ArgumentDoc>,
    options: Vec<OptionDoc>,
    output_schema: Option<&'static str>,
}

#[derive(Serialize)]
struct ArgumentDoc {
    name: String,
    required: bool,
    multiple: bool,
    help: String,
}

#[derive(Serialize)]
struct OptionDoc {
    long: String,
    short: Option<char>,
    value_name: Option<String>,
    required: bool,
    repeatable: bool,
    default: Option<String>,
    possible_values: Vec<String>,
    help: String,
}

fn describe(root: &Command) -> Description {
    Description {
        name: root.get_name().to_owned(),
        version: env!("CARGO_PKG_VERSION"),
        about: text(root.get_about()),
        global_options: options(root, true),
        exit_codes: EXIT_CODES
            .iter()
            .map(|&(code, meaning)| ExitCodeDoc { code, meaning })
            .collect(),
        error_schema: "error.schema.json",
        commands: root.get_subcommands().map(command).collect(),
    }
}

fn command(command: &Command) -> CommandDoc {
    let name = command.get_name();
    CommandDoc {
        name: name.to_owned(),
        about: text(command.get_about()),
        usage: command
            .clone()
            .render_usage()
            .to_string()
            .trim_start_matches("Usage: ")
            .to_owned(),
        arguments: command
            .get_arguments()
            .filter(|arg| arg.is_positional())
            .map(argument)
            .collect(),
        options: options(command, false),
        output_schema: output_schema(name),
    }
}

fn argument(arg: &Arg) -> ArgumentDoc {
    ArgumentDoc {
        name: value_name(arg).unwrap_or_else(|| arg.get_id().to_string().to_uppercase()),
        required: arg.is_required_set(),
        multiple: arg
            .get_num_args()
            .is_some_and(|range| range.max_values() > 1),
        help: text(arg.get_help()),
    }
}

/// The options of `command`; the global ones are listed once, on the top level.
fn options(command: &Command, global: bool) -> Vec<OptionDoc> {
    command
        .get_arguments()
        .filter(|arg| arg.get_long().is_some() && arg.is_global_set() == global)
        .filter(|arg| !matches!(arg.get_id().as_str(), "help" | "version"))
        .map(|arg| OptionDoc {
            long: arg.get_long().unwrap_or_default().to_owned(),
            short: arg.get_short(),
            value_name: arg.get_action().takes_values().then(|| {
                value_name(arg).unwrap_or_else(|| arg.get_id().to_string().to_uppercase())
            }),
            required: arg.is_required_set(),
            repeatable: matches!(arg.get_action(), ArgAction::Append),
            default: arg
                .get_action()
                .takes_values()
                .then(|| arg.get_default_values().first())
                .flatten()
                .map(|value| value.to_string_lossy().into_owned()),
            possible_values: arg
                .get_possible_values()
                .iter()
                .map(|value| value.get_name().to_owned())
                .collect(),
            help: text(arg.get_help()),
        })
        .collect()
}

fn value_name(arg: &Arg) -> Option<String> {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(ToString::to_string)
}

fn text(styled: Option<&clap::builder::StyledStr>) -> String {
    styled.map(ToString::to_string).unwrap_or_default()
}

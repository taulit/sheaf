//! The command line as clap sees it. `help --json` is derived from this tree.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Notes as plain Markdown files, for the desktop and for scripts.
///
/// Notes are addressed by id, exact title or unique title prefix. With --json every
/// command prints one JSON document ("schema": 1) and errors come as JSON on stderr.
/// Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 ambiguous.
#[derive(Debug, Parser)]
#[command(name = "sheaf", version, disable_help_subcommand = true)]
pub struct Cli {
    /// Notes folder [default: $SHEAF_NOTES_DIR, then the configuration, then ~/Documents/Notes]
    #[arg(long, global = true, value_name = "PATH")]
    pub dir: Option<PathBuf>,

    /// Print one JSON document instead of text
    #[arg(long, global = true)]
    pub json: bool,

    #[command(flatten)]
    pub app: AppArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// How to start the app; only without a command.
#[derive(Clone, Debug, Default, Args)]
pub struct AppArgs {
    /// Markdown files to open: a note in the notes window, any other file in a window
    /// of its own, edited where it is
    #[arg(value_name = "FILE")]
    pub files: Vec<String>,

    /// Start the app with a new note
    #[arg(long)]
    pub new: bool,

    /// Start the app with the Quick Note window
    #[arg(long, conflicts_with = "new")]
    pub quick: bool,

    /// Start the app without a window; it shows one when asked to over D-Bus
    #[arg(long, hide = true)]
    pub dbus_service: bool,
}

impl AppArgs {
    pub fn any(&self) -> bool {
        self.new || self.quick || self.dbus_service || !self.files.is_empty()
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a note and print its id
    New(NewArgs),
    /// Add text at the end of a note
    Append(TextArgs),
    /// Add text below the title of a note
    Prepend(TextArgs),
    /// Print a note
    Show(ShowArgs),
    /// Open a note in $VISUAL or $EDITOR
    Edit(NoteArg),
    /// Search with the query language of the app (#tag, folder:, has:, open:, …)
    Search(SearchArgs),
    /// List notes
    List(ListArgs),
    /// List checklist items of all notes or of one note
    Tasks(TasksArgs),
    /// Check a checklist item
    Check(TaskArgs),
    /// Uncheck a checklist item
    Uncheck(TaskArgs),
    /// List tags with the number of notes
    Tags,
    /// List folders, empty ones included, with the number of notes
    Folders,
    /// Move a note to a folder ("/" or "." is the notes folder itself)
    Mv(MvArgs),
    /// Move a note to the trash
    Rm(NoteArg),
    /// Take a note out of the trash
    Restore(NoteArg),
    /// Pin a note
    Pin(NoteArg),
    /// Unpin a note
    Unpin(NoteArg),
    /// List the notes in the trash
    Trash,
    /// Show a note in the app
    Open(NoteArg),
    /// Open the Quick Note window of the app
    Quick,
    /// Rebuild the search index
    Reindex,
    /// Print the notes folder
    Path,
    /// Print a shell completion script
    Completions(CompletionsArgs),
    /// Print help for a command; with --json, describe every command
    Help(HelpArgs),
}

#[derive(Debug, Args)]
pub struct NoteArg {
    /// Note id, exact title or unique title prefix
    #[arg(value_name = "NOTE")]
    pub note: String,
}

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Text of the note, first line is the title; "-" or no argument reads stdin
    #[arg(value_name = "TEXT", allow_hyphen_values = true)]
    pub text: Option<String>,

    /// Folder to create the note in, relative to the notes folder
    #[arg(long, value_name = "FOLDER")]
    pub folder: Option<String>,

    /// Start from this template; {{title}} is the first line of TEXT, the rest follows it
    #[arg(long, value_name = "TEMPLATE")]
    pub template: Option<String>,

    /// Add a tag (repeatable)
    #[arg(long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,

    /// Pin the note
    #[arg(long)]
    pub pin: bool,
}

#[derive(Debug, Args)]
pub struct TextArgs {
    #[command(flatten)]
    pub note: NoteArg,

    /// Text to add; "-" or no argument reads stdin
    #[arg(value_name = "TEXT", allow_hyphen_values = true)]
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Format {
    /// The note text without frontmatter
    Md,
    /// Metadata and body as JSON
    Json,
    /// Plain text without Markdown markers
    Text,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    #[command(flatten)]
    pub note: NoteArg,

    /// Output format; --json implies json
    #[arg(long, value_enum, default_value = "md")]
    pub format: Format,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search words and filters
    #[arg(required = true, num_args = 1.., value_name = "QUERY")]
    pub query: Vec<String>,

    /// Maximum number of results
    #[arg(long, value_name = "N", default_value_t = 20)]
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SortKey {
    Modified,
    Created,
    Title,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only notes in this folder (or a folder of this name)
    #[arg(long, value_name = "FOLDER")]
    pub folder: Option<String>,

    /// Only notes with this tag
    #[arg(long, value_name = "TAG")]
    pub tag: Option<String>,

    /// Only pinned notes
    #[arg(long)]
    pub pinned: bool,

    /// Only notes modified since this date (YYYY-MM-DD or RFC 3339)
    #[arg(long, value_name = "DATE")]
    pub modified_since: Option<String>,

    /// Order of the list
    #[arg(long, value_enum, default_value = "modified")]
    pub sort: SortKey,

    /// Maximum number of notes
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct TasksArgs {
    /// Only unchecked items
    #[arg(long)]
    pub open: bool,

    /// Only items of this note
    #[arg(long, value_name = "NOTE")]
    pub note: Option<String>,
}

#[derive(Debug, Args)]
pub struct TaskArgs {
    #[command(flatten)]
    pub note: NoteArg,

    /// Number of the item in the note (from 1), or text that exactly one item contains
    #[arg(value_name = "NUMBER|TEXT")]
    pub task: String,
}

#[derive(Debug, Args)]
pub struct MvArgs {
    #[command(flatten)]
    pub note: NoteArg,

    /// Target folder, relative to the notes folder
    #[arg(value_name = "FOLDER")]
    pub folder: String,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    #[arg(value_enum)]
    pub shell: Shell,
}

#[derive(Debug, Args)]
pub struct HelpArgs {
    /// Command to describe
    #[arg(value_name = "COMMAND")]
    pub command: Vec<String>,
}

//! `sheaf-searchd`: the only writer of the search index and the D-Bus face of Sheaf.

mod library;
mod logger;
mod service;

use std::process::ExitCode;
use std::time::Duration;

use sheaf_core::paths::Env;

use crate::library::Library;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

const USAGE: &str = "\
Usage: sheaf-searchd [--reindex | --version | --help]

Indexes the Sheaf notes folder and serves it on the session bus as
io.github.taulit.Sheaf.SearchProvider. Usually started by D-Bus activation.

Options:
  --reindex   Rebuild the search index without D-Bus, then exit
  --version   Print the version
  --help      Print this help
";

enum Mode {
    Serve,
    Reindex,
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mode = match (args.next().as_deref(), args.next()) {
        (None, _) => Mode::Serve,
        (Some("--reindex"), None) => Mode::Reindex,
        (Some("--version" | "-V"), None) => {
            println!("sheaf-searchd {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        (Some("--help" | "-h"), None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    sheaf_core::i18n::init();
    logger::init();

    match run(mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sheaf-searchd: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(mode: Mode) -> Result<(), Box<dyn std::error::Error>> {
    let env = Env::from_process();
    let notes_dir = env.notes_dir(None)?;
    std::fs::create_dir_all(&notes_dir)?;
    let notes_dir = notes_dir.canonicalize()?;
    let index_dir = env.index_dir(&notes_dir)?;
    match mode {
        Mode::Reindex => Ok(Library::open(&notes_dir, &index_dir)?.reindex()?),
        Mode::Serve => zruntime::block_on(service::serve(&notes_dir, &index_dir, idle_timeout())),
    }
}

fn idle_timeout() -> Duration {
    std::env::var("SHEAF_SEARCHD_IDLE_SECS")
        .ok()
        .and_then(|secs| secs.parse().ok())
        .map_or(IDLE_TIMEOUT, Duration::from_secs)
}

//! `sheaf-searchd`: the only writer of the search index and the D-Bus face of Sheaf.

mod indexer;
mod library;
mod logger;
mod service;

use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use sheaf_core::NoteStore;
use sheaf_core::paths::Env;

use crate::indexer::Op;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// One malloc arena for all threads and a fixed trim threshold, which also stops the
/// mmap threshold from growing: memory the searches freed goes back to the system
/// instead of staying with the idle service (docs/ARCHITECTURE.md, "Index").
const MALLOC_TUNABLES: &str = "glibc.malloc.arena_max=1:glibc.malloc.trim_threshold=131072";

const USAGE: &str = "\
Usage: sheaf-searchd [--reindex | --version | --help]

Indexes the Sheaf notes folder and serves it on the session bus as
io.github.taulit.Sheaf.SearchProvider. Usually started by D-Bus activation.

Options:
  --reindex   Rebuild the search index without D-Bus, then exit
  --version   Print the version
  --help      Print this help
";

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let [flag, notes, index] = args.as_slice()
        && flag == indexer::FLAG
    {
        init();
        let served = indexer::serve(Path::new(notes), Path::new(index));
        return exit_code(served.map_err(Into::into));
    }
    let args: Vec<&str> = args.iter().map(|arg| arg.to_str().unwrap_or("")).collect();
    let outcome = match args.as_slice() {
        [] => {
            restart_with_malloc_tunables();
            start(|notes, index| zruntime::block_on(service::serve(notes, index, idle_timeout())))
        }
        ["--reindex"] => start(reindex),
        ["--version" | "-V"] => {
            println!("sheaf-searchd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["--help" | "-h"] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    exit_code(outcome)
}

/// glibc reads `GLIBC_TUNABLES` only when a process starts, so the service starts itself
/// once more with [`MALLOC_TUNABLES`] added, however it was launched (D-Bus activation,
/// Flatpak, by hand). Malloc tunables set by the user are left alone. Returns only if
/// nothing is to be done or the restart failed; call it while there is one thread.
fn restart_with_malloc_tunables() {
    let set = std::env::var("GLIBC_TUNABLES").unwrap_or_default();
    if set.contains("glibc.malloc.") {
        return;
    }
    let tunables = if set.is_empty() {
        MALLOC_TUNABLES.to_owned()
    } else {
        format!("{set}:{MALLOC_TUNABLES}")
    };
    let mut args = std::env::args_os();
    let arg0 = args.next().unwrap_or_else(|| "sheaf-searchd".into());
    let failed = match std::env::current_exe() {
        Ok(exe) => Command::new(exe)
            .arg0(arg0)
            .args(args)
            .env("GLIBC_TUNABLES", tunables)
            .exec(),
        Err(err) => err,
    };
    eprintln!("sheaf-searchd: running without malloc tunables: {failed}");
}

fn exit_code(outcome: Outcome) -> ExitCode {
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sheaf-searchd: {err}");
            ExitCode::FAILURE
        }
    }
}

fn init() {
    sheaf_core::i18n::init();
    logger::init();
}

/// Resolves the notes folder (creating it) and its index folder, and runs `f` with them.
fn start(f: impl FnOnce(&Path, &Path) -> Outcome) -> Outcome {
    init();
    let env = Env::from_process();
    let notes_dir = env.notes_dir(None)?;
    std::fs::create_dir_all(&notes_dir)?;
    let notes_dir: PathBuf = notes_dir.canonicalize()?;
    let index_dir = env.index_dir(&notes_dir)?;
    f(&notes_dir, &index_dir)
}

fn reindex(notes_dir: &Path, index_dir: &Path) -> Outcome {
    let entries = NoteStore::new(notes_dir).walk()?;
    let ops = std::iter::once(Op::Clear)
        .chain(entries.into_iter().map(|entry| Op::Upsert(entry.rel_path)));
    indexer::run(notes_dir, index_dir, ops)?;
    Ok(())
}

fn idle_timeout() -> Duration {
    std::env::var("SHEAF_SEARCHD_IDLE_SECS")
        .ok()
        .and_then(|secs| secs.parse().ok())
        .map_or(IDLE_TIMEOUT, Duration::from_secs)
}

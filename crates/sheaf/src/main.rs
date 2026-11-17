//! `sheaf`: the notes app and its command line in one binary.

mod cli;

fn main() -> std::process::ExitCode {
    cli::run(std::env::args_os())
}

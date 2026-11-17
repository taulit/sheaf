//! Writes generated notes into a folder, to measure the service against:
//!
//! ```text
//! cargo run --release -p sheaf-searchd --example generate_notes -- <dir> [count]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use sheaf_core::testgen;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(dir) = args.next().map(PathBuf::from) else {
        eprintln!("usage: generate_notes <dir> [count]");
        return ExitCode::from(2);
    };
    let count = args
        .next()
        .and_then(|count| count.to_str()?.parse().ok())
        .unwrap_or(1000);
    match testgen::write_notes(&dir, &testgen::notes(count)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("generate_notes: {err}");
            ExitCode::FAILURE
        }
    }
}

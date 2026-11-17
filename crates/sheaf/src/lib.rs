//! `sheaf`: the notes app and its command line in one binary.
//!
//! [`cli::run`] is the entry point of the binary. With a subcommand it never touches
//! the GUI; without one it hands over to [`app::run`].

pub mod app;
pub mod cli;
mod searchd;

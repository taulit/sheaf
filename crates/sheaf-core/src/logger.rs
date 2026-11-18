//! Log records to stderr, each line headed by the program's name; the level comes from
//! `SHEAF_LOG` and defaults to `warn`. Other crates (tantivy is chatty) only get
//! through with warnings and errors.

use std::sync::OnceLock;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr;

static LOGGER: Stderr = Stderr;
static NAME: OnceLock<&'static str> = OnceLock::new();

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let ours = metadata.target().starts_with("sheaf");
        metadata.level() <= log::max_level() && (ours || metadata.level() <= Level::Warn)
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{}: {}: {}",
                NAME.get().copied().unwrap_or("sheaf"),
                record.level().as_str().to_ascii_lowercase(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Sends the log of this process, as `name`, to stderr. Only the first call counts.
pub fn init(name: &'static str) {
    let level = std::env::var("SHEAF_LOG")
        .ok()
        .and_then(|level| level.parse().ok())
        .unwrap_or(LevelFilter::Warn);
    if NAME.set(name).is_ok() && log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

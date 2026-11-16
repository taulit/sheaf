//! Log records to stderr; the level comes from `SHEAF_LOG` and defaults to `warn`.
//! Other crates (tantivy is chatty) only get through with warnings and errors.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr;

static LOGGER: Stderr = Stderr;

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let ours = metadata.target().starts_with("sheaf");
        metadata.level() <= log::max_level() && (ours || metadata.level() <= Level::Warn)
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "sheaf-searchd: {}: {}",
                record.level().as_str().to_ascii_lowercase(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let level = std::env::var("SHEAF_LOG")
        .ok()
        .and_then(|level| level.parse().ok())
        .unwrap_or(LevelFilter::Warn);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

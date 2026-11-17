//! The performance gate for search, run by CI:
//!
//! ```text
//! cargo run --release -p sheaf-core --features testgen --example search_budget
//! ```
//!
//! Builds an index of 10,000 generated notes on disk, the way the service does on its
//! first start, then runs the query mix of [`testgen::QUERIES`] against the warm index.
//! Fails if the build takes 10 s or more, or if the 95th percentile of all searches is
//! 30 ms or more.

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;
use sheaf_core::query::parse;
use sheaf_core::{SearchIndex, testgen};

const NOTES: usize = 10_000;
/// Hits per search, each with its snippet: more than a screenful of cards.
const HITS: usize = 50;
/// Measured runs of the whole mix, after one run to warm caches.
const ROUNDS: usize = 25;

const BUILD_BUDGET: Duration = Duration::from_secs(10);
const P95_BUDGET: Duration = Duration::from_millis(30);

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("search_budget: {error}");
            ExitCode::from(2)
        }
    }
}

/// Runs the measurement and tells whether both budgets were met.
fn run() -> sheaf_core::Result<bool> {
    let notes = testgen::notes(NOTES);
    let megabytes = |bytes: u64| bytes as f64 / 1e6;
    let text_bytes: u64 = notes.iter().map(|note| note.body.len() as u64).sum();

    let dir = tempfile::tempdir()?;
    let started = Instant::now();
    let index = SearchIndex::open_or_create(dir.path())?;
    let mut writer = index.writer()?;
    for note in &notes {
        writer.upsert(note)?;
    }
    writer.finish()?;
    let build = started.elapsed();
    let mut index_bytes = 0;
    for entry in std::fs::read_dir(dir.path())? {
        index_bytes += entry?.metadata()?.len();
    }
    println!(
        "{NOTES} notes, {:.1} MB of text; index {:.1} MB, built in {:.2} s (budget {} s)",
        megabytes(text_bytes),
        megabytes(index_bytes),
        build.as_secs_f64(),
        BUILD_BUDGET.as_secs(),
    );

    let reader = index.reader();
    let now = testgen::now().to_zoned_datetime(TimeZone::UTC);
    let mut classes: Vec<(&str, Vec<Duration>)> = Vec::new();
    let mut slowest = (Duration::ZERO, "");
    for round in 0..=ROUNDS {
        for (index, (class, queries)) in testgen::QUERIES.iter().enumerate() {
            if round == 0 {
                classes.push((class, Vec::new()));
            }
            for query in *queries {
                let started = Instant::now();
                let results = reader.search(&parse(query), HITS, &now)?;
                let elapsed = started.elapsed();
                black_box(results);
                if round > 0 {
                    classes[index].1.push(elapsed);
                    slowest = slowest.max((elapsed, query));
                }
            }
        }
    }

    println!(
        "\n{:<18} {:>7} {:>9} {:>9} {:>9}",
        "query class", "queries", "p50 ms", "p95 ms", "max ms"
    );
    let mut all = Vec::new();
    for (class, samples) in &mut classes {
        report(class, samples, ROUNDS);
        all.extend_from_slice(samples);
    }
    let p95 = report("all", &mut all, ROUNDS);
    println!(
        "\nslowest: {:?} with {:.2} ms",
        slowest.1,
        millis(slowest.0)
    );

    let build_ok = build < BUILD_BUDGET;
    let search_ok = p95 < P95_BUDGET;
    if !build_ok {
        println!(
            "FAILED: the index build took {:.2} s, the budget is {} s",
            build.as_secs_f64(),
            BUILD_BUDGET.as_secs()
        );
    }
    if !search_ok {
        println!(
            "FAILED: p95 is {:.2} ms, the budget is {} ms",
            millis(p95),
            P95_BUDGET.as_millis()
        );
    }
    Ok(build_ok && search_ok)
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

/// Prints one row of the table and returns the 95th percentile.
fn report(class: &str, samples: &mut [Duration], rounds: usize) -> Duration {
    samples.sort_unstable();
    let percentile = |percent: usize| samples[(samples.len() * percent).div_ceil(100).max(1) - 1];
    println!(
        "{class:<18} {:>7} {:>9.2} {:>9.2} {:>9.2}",
        samples.len() / rounds,
        millis(percentile(50)),
        millis(percentile(95)),
        millis(percentile(100)),
    );
    percentile(95)
}

//! Benchmarks of the index build and of search on 10,000 generated notes. A search
//! measurement runs every query of its class once, so divide by their number.
//!
//! ```text
//! cargo bench -p sheaf-core --features testgen
//! ```

#![allow(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use jiff::tz::TimeZone;
use sheaf_core::query::parse;
use sheaf_core::{NoteDoc, SearchIndex, testgen};

const NOTES: usize = 10_000;
const HITS: usize = 50;

fn build(notes: &[NoteDoc]) -> SearchIndex {
    let index = SearchIndex::in_memory().unwrap();
    let mut writer = index.writer().unwrap();
    for note in notes {
        writer.upsert(note).unwrap();
    }
    writer.finish().unwrap();
    index
}

fn index_build(criterion: &mut Criterion) {
    let notes = testgen::notes(NOTES);
    let mut group = criterion.benchmark_group("index");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(30));
    group.bench_function("build 10k notes", |bencher| {
        bencher.iter_batched(|| (), |()| build(&notes), BatchSize::PerIteration);
    });
    group.finish();
}

fn search(criterion: &mut Criterion) {
    let index = build(&testgen::notes(NOTES));
    let reader = index.reader();
    let now = testgen::now().to_zoned_datetime(TimeZone::UTC);
    let mut group = criterion.benchmark_group("search top 50 with snippets");
    for (class, queries) in testgen::QUERIES {
        group.bench_function(*class, |bencher| {
            bencher.iter(|| {
                for query in *queries {
                    black_box(reader.search(&parse(query), HITS, &now).unwrap());
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, index_build, search);
criterion_main!(benches);

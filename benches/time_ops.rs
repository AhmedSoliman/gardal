use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use gardal::{Clock, FastClock, QuantaClock, StdClock};

fn time_single_threaded(c: &mut Criterion) {
    // 1KHz
    let _quanta_thread = quanta::Upkeep::new(Duration::from_micros(10))
        .start()
        .unwrap();
    let mut group = c.benchmark_group("gardal");
    group
        .sample_size(100)
        .bench_function("std-time-getting-instant", |b| {
            b.iter(|| std::hint::black_box(StdClock.now()));
        })
        .bench_function("quanta-time-getting-instant", |b| {
            b.iter(|| std::hint::black_box(QuantaClock.now()));
        })
        .bench_function("quanta-fast-getting-instant", |b| {
            b.iter(|| std::hint::black_box(FastClock.now()));
        });
    group.finish();
}

criterion_group!(time_benches, time_single_threaded);
criterion_main!(time_benches);

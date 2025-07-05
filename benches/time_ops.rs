use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use gardal::{Clock, FastClock, QuantaClock, StdClock};

fn time_single_threaded(c: &mut Criterion) {
    let c_clock = quanta::Clock::new();
    // 1KHz
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_micros(10), c_clock.clone())
        .start()
        .unwrap();
    let mut group = c.benchmark_group("gardal");
    group
        .sample_size(100)
        .bench_function("std-time-getting-instant", |b| {
            let clock = StdClock::default();
            b.iter(|| clock.now());
        })
        .bench_function("quanta-time-getting-instant", |b| {
            let clock = QuantaClock::default();
            b.iter(|| clock.now());
        })
        .bench_function("quanta-fast-getting-instant", |b| {
            let clock = FastClock::new(c_clock.clone());
            b.iter(|| clock.now());
        });
    group.finish();
}

criterion_group!(time_benches, time_single_threaded);
criterion_main!(time_benches);

use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use gardal::{
    AtomicStorage, FastClock, Limit, LocalStorage, ManualClock, PaddedAtomicStorage, QuantaClock,
    StdClock, TokenBucket,
};
use nonzero_ext::nonzero;

fn bench_consume(c: &mut Criterion) {
    let clock = quanta::Clock::new();
    let limit = Limit::per_second(nonzero!(10_000u32));
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_micros(10), clock.clone())
        .start()
        .unwrap();
    let clock = FastClock::new(clock);
    let quanta_tb =
        TokenBucket::<PaddedAtomicStorage, _>::from_parts(limit, QuantaClock::default());
    let std_tb = TokenBucket::<PaddedAtomicStorage, _>::from_parts(limit, StdClock::default());
    let fast_tb = TokenBucket::<LocalStorage, _>::from_parts(limit, clock.clone());
    let fast_tb_padded = TokenBucket::<PaddedAtomicStorage, _>::from_parts(limit, clock.clone());
    std::thread::sleep(Duration::from_secs(1));
    let mut group = c.benchmark_group("tokenbucket");
    group
        .throughput(Throughput::Elements(1))
        .sample_size(100)
        .bench_function("consume-mock-clock-local-storage", |b| {
            let clock = ManualClock::default();
            let tb = TokenBucket::<LocalStorage, _>::from_parts(limit, &clock);
            clock.set(10.0);
            b.iter(|| {
                let _x = std::hint::black_box(tb.try_consume_one());
            });
        })
        .bench_function("consume-mock-clock-atomic-storage", |b| {
            let clock = ManualClock::default();
            let tb = TokenBucket::<AtomicStorage, _>::from_parts(limit, &clock);
            clock.set(10.0);
            b.iter(|| {
                tb.consume_one();
            });
        })
        .bench_function("consume-mock-clock-padded-atomic-storage", |b| {
            let clock = ManualClock::default();
            let tb = TokenBucket::<PaddedAtomicStorage, _>::from_parts(limit, &clock);
            clock.set(10.0);
            b.iter(|| {
                tb.consume_one();
            });
        })
        .bench_function("consume-std-clock-padded-atomic-storage", |b| {
            b.iter(|| {
                std_tb.consume_one();
            });
        })
        .bench_function("consume-fast-clock-local-storage", |b| {
            b.iter(|| {
                fast_tb.consume_one();
            });
        })
        .bench_function("consume-fast-clock-padded-atomic-storage", |b| {
            b.iter(|| {
                fast_tb_padded.consume_one();
            })
        })
        .bench_function("consume-quanta-clock-padded-atomic-storage", |b| {
            b.iter(|| {
                quanta_tb.consume_one();
            });
        });
    group.finish();
}

const THREADS: u32 = 24;

fn multi_threaded(c: &mut Criterion) {
    let clock = quanta::Clock::new();
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_micros(100), clock.clone())
        .start()
        .unwrap();
    let clock = FastClock::new(clock);
    let limit = Limit::per_second(nonzero!(10_000u32));
    let mut group = c.benchmark_group("multi_threaded");
    group
        .throughput(Throughput::Elements(1))
        .bench_function("padded", |b| {
            let tb = Arc::new(TokenBucket::<PaddedAtomicStorage, _>::from_parts(
                limit,
                clock.clone(),
            ));
            b.iter_custom(|iters| {
                let mut children = vec![];
                let start = std::time::Instant::now();
                for _i in 0..THREADS {
                    let tb = Arc::clone(&tb);
                    children.push(std::thread::spawn(move || {
                        for _i in 0..iters {
                            //std::hint::black_box(tb.saturating_consume_with_borrow(1.0));
                            let _x = std::hint::black_box(tb.consume_with_borrow(nonzero!(1u32)));
                        }
                    }));
                }
                for child in children {
                    child.join().unwrap()
                }
                start.elapsed()
            })
        })
        .bench_function("atomic", |b| {
            let tb = Arc::new(TokenBucket::<AtomicStorage, _>::from_parts(
                limit,
                clock.clone(),
            ));
            b.iter_custom(|iters| {
                let mut children = vec![];
                let start = std::time::Instant::now();
                for _i in 0..THREADS {
                    let tb = Arc::clone(&tb);
                    children.push(std::thread::spawn(move || {
                        for _i in 0..iters {
                            let _x = std::hint::black_box(tb.consume_with_borrow(nonzero!(1u32)));
                        }
                    }));
                }
                for child in children {
                    child.join().unwrap()
                }
                start.elapsed()
            })
        });
    group.finish();
}

fn multi_threaded2(c: &mut Criterion) {
    let clock = quanta::Clock::new();
    let _quanta_thread = quanta::Upkeep::new_with_clock(Duration::from_micros(10), clock.clone())
        .start()
        .unwrap();
    let clock = FastClock::new(clock);
    let limit = Limit::per_second(nonzero!(50u32));
    let mut group = c.benchmark_group("multi_threaded2");
    group
        .throughput(Throughput::Elements(1))
        .bench_function("padded", |b| {
            b.iter_custom(|iters| {
                let tb = Arc::new(TokenBucket::<PaddedAtomicStorage, _>::from_parts(
                    limit,
                    clock.clone(),
                ));
                let mut children = vec![];
                let start = std::time::Instant::now();
                for _i in 0..THREADS {
                    let tb = Arc::clone(&tb);
                    children.push(std::thread::spawn(move || {
                        for _i in 0..iters {
                            std::hint::black_box(tb.try_consume_one().is_ok());
                        }
                    }));
                }
                for child in children {
                    child.join().unwrap()
                }
                start.elapsed()
            })
        })
        .bench_function("atomic", |b| {
            b.iter_custom(|iters| {
                let tb = Arc::new(TokenBucket::<AtomicStorage, _>::from_parts(
                    limit,
                    clock.clone(),
                ));
                let mut children = vec![];
                let start = std::time::Instant::now();
                for _i in 0..THREADS {
                    let tb = Arc::clone(&tb);
                    children.push(std::thread::spawn(move || {
                        for _i in 0..iters {
                            std::hint::black_box(tb.try_consume_one().is_ok());
                        }
                    }));
                }
                for child in children {
                    child.join().unwrap()
                }
                start.elapsed()
            })
        });
    group.finish();
}

criterion_group!(benches, bench_consume, multi_threaded, multi_threaded2);
criterion_main!(benches);

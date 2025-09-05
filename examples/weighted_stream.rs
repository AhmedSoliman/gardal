use futures::stream;
use gardal::futures::{StreamExt as GardalStreamExt, WeightedStream};
use gardal::{Limit, LocalStorage, TokenBucket, TokioClock};
use nonzero_ext::nonzero;
use std::num::NonZeroU32;
use tokio_stream::StreamExt;

#[derive(Debug, Clone)]
struct Task {
    name: String,
    size: usize,
}

impl Task {
    fn new(name: &str, size: usize) -> Self {
        Self {
            name: name.to_string(),
            size,
        }
    }
}

#[tokio::main]
async fn main() {
    println!("WeightedStream Example");
    println!("=====================");

    // Create a stream of tasks with different sizes
    let tasks = vec![
        Task::new("small_task", 1),
        Task::new("medium_task", 5),
        Task::new("large_task", 10),
        Task::new("tiny_task", 1),
        Task::new("huge_task", 20),
    ];

    let stream = stream::iter(tasks);

    // Create a throttling limit: 5 tokens per second with a burst of 25
    let limit = Limit::per_second_and_burst(nonzero!(5u32), nonzero!(25u32));
    let bucket = TokenBucket::<LocalStorage, _>::from_parts(limit, TokioClock::default());

    // Create a weighted stream where each task consumes tokens based on its size
    let weighted_stream = WeightedStream::new(stream, bucket, |task: &Task| {
        // Each task consumes tokens equal to its size
        NonZeroU32::new(task.size as u32).unwrap_or(nonzero!(1u32))
    });

    println!("Processing tasks with weighted throttling...");
    println!("Throttling: 5 tokens/second, burst: 25 tokens");
    println!();

    let start = std::time::Instant::now();
    let mut weighted_stream = std::pin::pin!(weighted_stream);

    while let Some(task) = weighted_stream.next().await {
        let elapsed = start.elapsed();
        println!(
            "[{:>6.2}s] Processed task '{}' (size: {} tokens)",
            elapsed.as_secs_f64(),
            task.name,
            task.size
        );
    }

    let total_elapsed = start.elapsed();
    println!();
    println!("Total time: {:.2}s", total_elapsed.as_secs_f64());

    // Demonstrate using the extension trait
    println!();
    println!("Using extension trait:");
    println!("=====================");

    let tasks2 = vec!["short", "medium_length", "very_long_string_here", "x"];

    let stream2 = stream::iter(tasks2);
    let limit2 = Limit::per_second_and_burst(nonzero!(3u32), nonzero!(25u32));
    let bucket2 = TokenBucket::<LocalStorage, _>::from_parts(limit2, TokioClock::default());

    // Use the extension trait to create a weighted stream
    let weighted_stream2 = stream2.throttle_weighted(bucket2, |text: &&str| {
        // Consume tokens based on string length
        NonZeroU32::new(text.len() as u32).unwrap_or(nonzero!(1u32))
    });

    let start2 = std::time::Instant::now();
    let mut weighted_stream2 = std::pin::pin!(weighted_stream2);

    while let Some(text) = weighted_stream2.next().await {
        let elapsed = start2.elapsed();
        println!(
            "[{:>6.2}s] Processed text '{}' (length: {} tokens)",
            elapsed.as_secs_f64(),
            text,
            text.len()
        );
    }

    let total_elapsed2 = start2.elapsed();
    println!();
    println!("Total time: {:.2}s", total_elapsed2.as_secs_f64());
}

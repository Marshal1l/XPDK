use lockfree_ringbuf::MpmcRingBuffer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== MPMC (Multi Producer Multi Consumer) Example ===");

    // Create a ring buffer with capacity 1024
    let rb = Arc::new(MpmcRingBuffer::new(1024));
    let stop_signal = Arc::new(AtomicBool::new(false));

    // Spawn multiple producer threads
    let mut producers = vec![];
    for producer_id in 0..3 {
        let rb_producer = Arc::clone(&rb);
        let producer = thread::spawn(move || {
            println!("Producer {}: Starting...", producer_id);

            for i in 0..30 {
                let message = format!("P{}-M{}", producer_id, i);

                // Try to push the message
                while rb_producer.push(message.clone()).is_err() {
                    thread::yield_now();
                }

                if i % 10 == 0 {
                    println!("Producer {}: Sent '{}'", producer_id, message);
                }

                // Small delay to make things more interesting
                thread::sleep(std::time::Duration::from_millis(5));
            }

            println!("Producer {}: Finished", producer_id);
        });
        producers.push(producer);
    }

    // Spawn multiple consumer threads
    let mut consumers = vec![];
    for consumer_id in 0..2 {
        let rb_consumer = Arc::clone(&rb);
        let stop_consumer = Arc::clone(&stop_signal);

        let consumer = thread::spawn(move || {
            println!("Consumer {}: Starting...", consumer_id);
            let mut consumed = 0;

            loop {
                match rb_consumer.pop() {
                    Ok(message) => {
                        consumed += 1;
                        if consumed % 15 == 0 {
                            println!(
                                "Consumer {}: Received '{}' (total: {})",
                                consumer_id, message, consumed
                            );
                        }
                    }
                    Err(_) => {
                        // Buffer is empty, check if all producers are done
                        if stop_consumer.load(Ordering::Relaxed) {
                            // Try one more time to get any remaining messages
                            if rb_consumer.pop().is_ok() {
                                consumed += 1;
                                continue;
                            } else {
                                break;
                            }
                        }
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }

            println!(
                "Consumer {}: Finished, consumed {} messages",
                consumer_id, consumed
            );
            consumed
        });
        consumers.push(consumer);
    }

    // Wait for all producers to complete
    for producer in producers {
        producer.join().unwrap();
    }

    // Signal consumers that production is done
    stop_signal.store(true, Ordering::Relaxed);

    // Wait for all consumers to complete and collect results
    let mut total_consumed = 0;
    for consumer in consumers {
        total_consumed += consumer.join().unwrap();
    }

    println!("Total messages consumed: {}", total_consumed);
    println!("Expected messages: 90"); // 3 producers * 30 messages each
    println!("=== MPMC Example Complete ===");
}

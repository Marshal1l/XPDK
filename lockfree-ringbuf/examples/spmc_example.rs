use lockfree_ringbuf::SpmcRingBuffer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== SPMC (Single Producer Multi Consumer) Example ===");

    // Create a ring buffer with capacity 1024
    let rb = Arc::new(SpmcRingBuffer::new(1024));
    let stop_signal = Arc::new(AtomicBool::new(false));

    // Single producer thread
    let rb_producer = Arc::clone(&rb);
    let stop_producer = Arc::clone(&stop_signal);
    let producer = thread::spawn(move || {
        println!("Producer: Starting to produce messages...");

        for i in 0..100 {
            let message = format!("Message {}", i);

            // Try to push the message
            while rb_producer.push(message.clone()).is_err() {
                thread::yield_now();
            }

            if i % 20 == 0 {
                println!("Producer: Sent '{}' (total: {})", message, i + 1);
            }

            // Small delay to make consumption more interesting
            thread::sleep(std::time::Duration::from_millis(10));
        }

        println!("Producer: Finished producing 100 messages");
        stop_producer.store(true, Ordering::Relaxed);
    });

    // Spawn multiple consumer threads
    let mut consumers = vec![];
    for consumer_id in 0..3 {
        let rb_consumer = Arc::clone(&rb);
        let stop_consumer = Arc::clone(&stop_signal);

        let consumer = thread::spawn(move || {
            println!("Consumer {}: Starting...", consumer_id);
            let mut consumed = 0;

            while !stop_consumer.load(Ordering::Relaxed) {
                match rb_consumer.pop() {
                    Ok(message) => {
                        consumed += 1;
                        if consumed % 10 == 0 {
                            println!(
                                "Consumer {}: Received '{}' (total: {})",
                                consumer_id, message, consumed
                            );
                        }
                    }
                    Err(_) => {
                        // Buffer is empty, check stop signal
                        if stop_consumer.load(Ordering::Relaxed) {
                            // Try one more time to get any remaining messages
                            if rb_consumer.pop().is_ok() {
                                consumed += 1;
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

    // Wait for producer to complete
    producer.join().unwrap();

    // Wait for all consumers to complete and collect results
    let mut total_consumed = 0;
    for consumer in consumers {
        total_consumed += consumer.join().unwrap();
    }

    println!("Total messages consumed: {}", total_consumed);
    println!("=== SPMC Example Complete ===");
}

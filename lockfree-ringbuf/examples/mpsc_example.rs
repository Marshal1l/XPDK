use lockfree_ringbuf::MpscRingBuffer;
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== MPSC (Multi Producer Single Consumer) Example ===");

    // Create a ring buffer with capacity 1024
    let rb = Arc::new(MpscRingBuffer::new(1024));
    let rb_clone = Arc::clone(&rb);

    // Spawn multiple producer threads
    let mut producers = vec![];
    for producer_id in 0..3 {
        let rb_clone = Arc::clone(&rb);
        let producer = thread::spawn(move || {
            println!("Producer {}: Starting...", producer_id);

            for i in 0..20 {
                let message = format!("P{}-M{}", producer_id, i);

                // Try to push the message
                while rb_clone.push(message.clone()).is_err() {
                    thread::yield_now();
                }

                if i % 5 == 0 {
                    println!("Producer {}: Sent message '{}'", producer_id, message);
                }
            }

            println!("Producer {}: Finished", producer_id);
        });
        producers.push(producer);
    }

    // Single consumer thread
    let consumer = thread::spawn(move || {
        println!("Consumer: Starting to consume messages...");
        let mut consumed = 0;

        while consumed < 60 {
            match rb.pop() {
                Ok(message) => {
                    consumed += 1;
                    if consumed % 10 == 0 {
                        println!("Consumer: Received '{}' (total: {})", message, consumed);
                    }
                }
                Err(_) => {
                    // Buffer is empty, wait a bit
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }

        println!("Consumer: Finished consuming {} messages", consumed);
    });

    // Wait for all producers to complete
    for producer in producers {
        producer.join().unwrap();
    }

    // Wait for consumer to complete
    consumer.join().unwrap();

    println!("=== MPSC Example Complete ===");
}

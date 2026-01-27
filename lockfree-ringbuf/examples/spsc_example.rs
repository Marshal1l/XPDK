use lockfree_ringbuf::SpscRingBuffer;
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== SPSC (Single Producer Single Consumer) Example ===");

    // Create a ring buffer with capacity 1024
    let rb = Arc::new(SpscRingBuffer::new(1024));
    let rb_clone = Arc::clone(&rb);

    // Producer thread
    let producer = thread::spawn(move || {
        println!("Producer: Starting to produce messages...");

        for i in 0..100 {
            let message = format!("Message {}", i);

            // Try to push the message
            match rb_clone.push(message) {
                Ok(()) => {
                    if i % 10 == 0 {
                        println!("Producer: Sent {} messages", i + 1);
                    }
                }
                Err(_) => {
                    println!("Producer: Buffer full, waiting...");
                    while rb_clone.push(format!("Message {}", i)).is_err() {
                        thread::yield_now();
                    }
                }
            }
        }

        println!("Producer: Finished producing 100 messages");
    });

    // Consumer thread
    let consumer = thread::spawn(move || {
        println!("Consumer: Starting to consume messages...");
        let mut consumed = 0;

        while consumed < 100 {
            match rb.pop() {
                Ok(message) => {
                    consumed += 1;
                    if consumed % 10 == 0 {
                        println!("Consumer: Received '{}'", message);
                    }
                }
                Err(_) => {
                    // Buffer is empty, wait a bit
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }

        println!("Consumer: Finished consuming 100 messages");
    });

    // Wait for both threads to complete
    producer.join().unwrap();
    consumer.join().unwrap();

    println!("=== SPSC Example Complete ===");
}

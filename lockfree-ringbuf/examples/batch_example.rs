use lockfree_ringbuf::{BatchOps, SpscRingBuffer};

fn main() {
    println!("=== Batch Operations Example ===");

    // Create a ring buffer
    let rb = SpscRingBuffer::new(1024);

    // Individual push/pop operations
    println!("1. Individual operations:");
    for i in 0..10 {
        rb.push(i).unwrap();
    }
    println!("   Pushed 10 individual items");

    for i in 0..10 {
        let value = rb.pop().unwrap();
        println!("   Popped: {}", value);
    }

    // Batch push operations
    println!("\n2. Batch push operations:");
    let batch_data: Vec<i32> = (100..110).collect();
    rb.push_batch(&batch_data).unwrap();
    println!(
        "   Pushed batch of {} items: {:?}",
        batch_data.len(),
        batch_data
    );

    // Batch pop operations
    println!("\n3. Batch pop operations:");
    let mut buffer = [0; 15]; // Larger than what's in the buffer
    let count = rb.pop_batch(&mut buffer).unwrap();
    println!("   Popped {} items in batch", count);
    println!("   Buffer contents: {:?}", &buffer[..count]);

    // Mixed individual and batch operations
    println!("\n4. Mixed operations:");

    // Push some individual items
    rb.push(1).unwrap();
    rb.push(2).unwrap();
    println!("   Pushed individual items: 1, 2");

    // Push a batch
    let batch = vec![3, 4, 5, 6];
    rb.push_batch(&batch).unwrap();
    println!("   Pushed batch: {:?}", batch);

    // Push more individual items
    rb.push(7).unwrap();
    rb.push(8).unwrap();
    println!("   Pushed individual items: 7, 8");

    // Pop everything in batches
    let mut all_items = vec![];
    loop {
        let mut temp_buffer = [0; 3];
        match rb.pop_batch(&mut temp_buffer) {
            Ok(count) => {
                all_items.extend_from_slice(&temp_buffer[..count]);
                println!(
                    "   Batch pop: got {} items: {:?}",
                    count,
                    &temp_buffer[..count]
                );
            }
            Err(_) => break,
        }
    }

    println!("\n   All items retrieved: {:?}", all_items);
    println!("   Total items: {}", all_items.len());

    // Performance comparison
    println!("\n5. Performance comparison:");

    // Individual operations
    let start = std::time::Instant::now();
    for i in 0..1000 {
        rb.push(i).unwrap();
    }
    for _ in 0..1000 {
        rb.pop().unwrap();
    }
    let individual_time = start.elapsed();

    // Batch operations
    let batch_data: Vec<i32> = (0..1000).collect();
    let start = std::time::Instant::now();
    rb.push_batch(&batch_data).unwrap();
    let mut buffer = [0; 1000];
    rb.pop_batch(&mut buffer).unwrap();
    let batch_time = start.elapsed();

    println!("   Individual operations: {:?}", individual_time);
    println!("   Batch operations: {:?}", batch_time);
    println!(
        "   Speedup: {:.2}x",
        individual_time.as_nanos() as f64 / batch_time.as_nanos() as f64
    );

    println!("\n=== Batch Operations Example Complete ===");
}

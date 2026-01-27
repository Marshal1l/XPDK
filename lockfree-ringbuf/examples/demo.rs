use lockfree_ringbuf::{BatchOps, MpmcRingBuffer, MpscRingBuffer, SpmcRingBuffer, SpscRingBuffer};

fn main() {
    println!("=== Lock-Free Ring Buffer Demo ===\n");

    // SPSC Demo
    println!("1. SPSC (Single Producer Single Consumer):");
    let spsc: SpscRingBuffer<i32> = SpscRingBuffer::new(8);
    spsc.push(1).unwrap();
    spsc.push(2).unwrap();
    spsc.push(3).unwrap();
    println!("   Pushed: 1, 2, 3");
    println!(
        "   Popped: {} {} {}",
        spsc.pop().unwrap(),
        spsc.pop().unwrap(),
        spsc.pop().unwrap()
    );
    println!("   Empty: {}", spsc.is_empty());

    // MPSC Demo
    println!("\n2. MPSC (Multi Producer Single Consumer):");
    let mpsc: MpscRingBuffer<i32> = MpscRingBuffer::new(8);
    mpsc.push(10).unwrap();
    mpsc.push(20).unwrap();
    println!("   Pushed: 10, 20");
    println!("   Length: {}", mpsc.len());
    println!("   Popped: {} {}", mpsc.pop().unwrap(), mpsc.pop().unwrap());

    // SPMC Demo
    println!("\n3. SPMC (Single Producer Multi Consumer):");
    let spmc: SpmcRingBuffer<i32> = SpmcRingBuffer::new(8);
    spmc.push(100).unwrap();
    spmc.push(200).unwrap();
    println!("   Pushed: 100, 200");
    println!("   Full: {}", spmc.is_full());
    println!("   Popped: {} {}", spmc.pop().unwrap(), spmc.pop().unwrap());

    // MPMC Demo
    println!("\n4. MPMC (Multi Producer Multi Consumer):");
    let mpmc: MpmcRingBuffer<i32> = MpmcRingBuffer::new(8);
    mpmc.push(1000).unwrap();
    mpmc.push(2000).unwrap();
    println!("   Pushed: 1000, 2000");
    println!("   Capacity: {}", mpmc.capacity());
    println!("   Popped: {} {}", mpmc.pop().unwrap(), mpmc.pop().unwrap());

    // Batch Operations Demo
    println!("\n5. Batch Operations:");
    let batch_rb: SpscRingBuffer<i32> = SpscRingBuffer::new(16);
    let batch_data = [1, 2, 3, 4, 5];
    batch_rb.push_batch(&batch_data).unwrap();
    println!("   Pushed batch: {:?}", batch_data);
    println!("   Length after batch: {}", batch_rb.len());

    let mut output = [0; 10];
    let count = batch_rb.pop_batch(&mut output).unwrap();
    println!("   Popped batch: {} items", count);
    println!("   Output: {:?}", &output[..count]);

    println!("\n=== Demo Complete ===");
    println!("\nKey Features:");
    println!("✓ Lock-free operations using atomic primitives");
    println!("✓ Support for SPSC/MPSC/SPMC/MPMC patterns");
    println!("✓ Batch operations for improved throughput");
    println!("✓ Cache-friendly memory layout");
    println!("✓ Power-of-2 capacity for fast modulo operations");
    println!("✓ no_std compatible for embedded use");
}

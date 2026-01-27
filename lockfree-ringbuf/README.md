# Lock-Free Ring Buffer

A high-performance, lock-free ring buffer implementation in Rust that supports multiple producer-consumer patterns with batch operations.

## Features

- **Lock-free**: No mutexes or locks, using only atomic operations
- **Multiple patterns**: SPSC, MPSC, SPMC, and MPMC support
- **Batch operations**: Efficient bulk push/pop operations
- **Cache-friendly**: Uses cache-padded atomic variables to reduce false sharing
- **Power-of-2 capacity**: Automatically rounds capacity to next power of 2 for fast modulo operations
- **`no_std` support**: Can be used in embedded environments

## Producer-Consumer Patterns

| Pattern | Description | Use Case |
|---------|-------------|---------|
| **SPSC** | Single Producer, Single Consumer | Fastest, ideal for pipelines |
| **MPSC** | Multi Producer, Single Consumer | Work queues, logging systems |
| **SPMC** | Single Producer, Multi Consumers | Fan-out patterns, broadcasting |
| **MPMC** | Multi Producer, Multi Consumer | General purpose concurrent queues |

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
lockfree-ringbuf = "0.1.0"
```

### Basic Examples

#### SPSC (Single Producer Single Consumer)

```rust
use lockfree_ringbuf::SpscRingBuffer;
use std::sync::Arc;
use std::thread;

let rb = Arc::new(SpscRingBuffer::new(1024));
let rb_clone = Arc::clone(&rb);

// Producer thread
let producer = thread::spawn(move || {
    for i in 0..100 {
        rb_clone.push(i).unwrap();
    }
});

// Consumer thread
let consumer = thread::spawn(move || {
    let mut sum = 0;
    for _ in 0..100 {
        sum += rb.pop().unwrap();
    }
    sum
});

producer.join().unwrap();
let result = consumer.join().unwrap();
println!("Sum: {}", result);
```

#### MPSC (Multi Producer Single Consumer)

```rust
use lockfree_ringbuf::MpscRingBuffer;
use std::sync::Arc;
use std::thread;

let rb = Arc::new(MpscRingBuffer::new(1024));

// Multiple producers
for producer_id in 0..3 {
    let rb_clone = Arc::clone(&rb);
    thread::spawn(move || {
        for i in 0..100 {
            let value = producer_id * 1000 + i;
            rb_clone.push(value).unwrap();
        }
    });
}

// Single consumer
let mut received = vec![];
while received.len() < 300 {
    if let Ok(value) = rb.pop() {
        received.push(value);
    }
}
```

#### Batch Operations

```rust
use lockfree_ringbuf::{SpscRingBuffer, BatchOps};

let rb = SpscRingBuffer::new(1024);

// Batch push
let items = vec![1, 2, 3, 4, 5];
rb.push_batch(&items).unwrap();

// Batch pop
let mut buffer = [0; 10];
let count = rb.pop_batch(&mut buffer).unwrap();
println!("Received {} items: {:?}", count, &buffer[..count]);
```

## Performance

The ring buffer is designed for high performance:

- **SPSC**: Fastest variant with minimal atomic operations
- **MPSC/SPMC**: Uses compare-and-swap for coordination
- **MPMC**: Full concurrent access with backoff strategies

### Benchmarks

Run the benchmarks to see performance on your system:

```bash
cargo bench
```

Typical results (single thread, push/pop operations):
- SPSC: ~10-20 million ops/sec
- MPSC: ~5-10 million ops/sec  
- SPMC: ~5-10 million ops/sec
- MPMC: ~3-8 million ops/sec

## API Reference

### Common Methods

All ring buffer types implement these common methods:

- `new(capacity: usize) -> Self`: Create a new ring buffer
- `capacity(&self) -> usize`: Get the buffer capacity
- `push(&self, value: T) -> Result<(), Error>`: Push a single value
- `pop(&self) -> Result<T, Error>`: Pop a single value
- `is_empty(&self) -> bool`: Check if buffer is empty
- `is_full(&self) -> bool`: Check if buffer is full
- `len(&self) -> usize`: Get current number of items

### Batch Operations

Implement the `BatchOps` trait:

- `push_batch(&self, items: &[T]) -> Result<(), Error>`: Push multiple items
- `pop_batch(&self, buf: &mut [T]) -> Result<usize, Error>`: Pop multiple items

### Error Types

```rust
pub enum Error {
    Full,   // Buffer is full
    Empty,  // Buffer is empty
}
```

## Thread Safety

All ring buffer variants are `Send + Sync` when `T: Send + Sync`:

- **SPSC**: Safe for one producer and one consumer
- **MPSC**: Safe for multiple producers and one consumer  
- **SPMC**: Safe for one producer and multiple consumers
- **MPMC**: Safe for multiple producers and multiple consumers

⚠️ **Important**: Using the wrong pattern (e.g., multiple producers with SPSC) can cause data races and undefined behavior.

## Implementation Details

### Memory Layout

The ring buffer uses:
- Cache-padded atomic indices to prevent false sharing
- Power-of-2 capacity for fast modulo operations using bitmask
- Unsafe memory operations for maximum performance

### Memory Ordering

- **Producer operations**: Use `Release` on tail updates, `Acquire` on head reads
- **Consumer operations**: Use `Release` on head updates, `Acquire` on tail reads
- **Compare-and-swap**: Uses `AcqRel` for proper synchronization

### Backoff Strategy

For contended operations (MPSC/SPMC/MPMC), the implementation uses an exponential backoff strategy from the `crossbeam` crate to reduce CPU contention.

## Examples

See the `examples/` directory for complete examples:

- `spsc_example.rs`: Single producer single consumer
- `mpsc_example.rs`: Multi producer single consumer  
- `spmc_example.rs`: Single producer multi consumer
- `mpmc_example.rs`: Multi producer multi consumer
- `batch_example.rs`: Batch operations demonstration

Run examples with:

```bash
cargo run --example spsc_example
cargo run --example mpsc_example
cargo run --example spmc_example
cargo run --example mpmc_example
cargo run --example batch_example
```

## Testing

Run tests with:

```bash
cargo test
```

The test suite includes:
- Unit tests for all ring buffer types
- Concurrent tests with multiple threads
- Batch operation tests
- Edge case tests (empty, full, wraparound)

## License

This project is licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Acknowledgments

- Inspired by well-known lock-free queue algorithms
- Uses `crossbeam-utils` for backoff strategies
- Uses `cache-padded` for cache-line alignment
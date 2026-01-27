use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lockfree_ringbuf::{BatchOps, MpmcRingBuffer, MpscRingBuffer, SpmcRingBuffer, SpscRingBuffer};
use std::sync::Arc;
use std::thread;

fn bench_spsc_single_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("spsc_single_thread");

    group.bench_function("push_pop", |b| {
        b.iter(|| {
            let rb = SpscRingBuffer::new(1024);
            for i in 0..1000 {
                black_box(rb.push(black_box(i))).unwrap();
            }
            for _ in 0..1000 {
                black_box(rb.pop()).unwrap();
            }
        })
    });

    group.bench_function("batch_operations", |b| {
        b.iter(|| {
            let rb = SpscRingBuffer::new(1024);
            let items: Vec<i32> = (0..100).collect();
            black_box(rb.push_batch(black_box(&items))).unwrap();

            let mut buf = [0; 100];
            let count = black_box(rb.pop_batch(black_box(&mut buf))).unwrap();
            assert_eq!(count, 100);
        })
    });

    group.finish();
}

fn bench_spsc_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("spsc_concurrent");

    group.bench_function("producer_consumer", |b| {
        b.iter(|| {
            let rb = Arc::new(SpscRingBuffer::new(1024));
            let rb_clone = Arc::clone(&rb);

            let producer = thread::spawn(move || {
                for i in 0..10000 {
                    while rb_clone.push(black_box(i)).is_err() {
                        thread::yield_now();
                    }
                }
            });

            let consumer = thread::spawn(move || {
                let mut sum = 0;
                for _ in 0..10000 {
                    while let Ok(value) = rb.pop() {
                        sum += black_box(value);
                    }
                }
                sum
            });

            producer.join().unwrap();
            black_box(consumer.join().unwrap());
        })
    });

    group.finish();
}

fn bench_mpsc_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_concurrent");

    group.bench_function("multiple_producers", |b| {
        b.iter(|| {
            let rb = Arc::new(MpscRingBuffer::new(1024));
            let mut handles = vec![];

            // Spawn 4 producers
            for i in 0..4 {
                let rb_clone = Arc::clone(&rb);
                let handle = thread::spawn(move || {
                    for j in 0..2500 {
                        let value = i * 2500 + j;
                        while rb_clone.push(black_box(value)).is_err() {
                            thread::yield_now();
                        }
                    }
                });
                handles.push(handle);
            }

            // Spawn 1 consumer
            let rb_clone = Arc::clone(&rb);
            let consumer = thread::spawn(move || {
                let mut count = 0;
                while count < 10000 {
                    if rb.pop().is_ok() {
                        count += 1;
                    }
                }
            });

            for handle in handles {
                handle.join().unwrap();
            }
            consumer.join().unwrap();
        })
    });

    group.finish();
}

fn bench_spmc_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("spmc_concurrent");

    group.bench_function("multiple_consumers", |b| {
        b.iter(|| {
            let rb = Arc::new(SpmcRingBuffer::new(1024));
            let mut handles = vec![];

            // Spawn 1 producer
            let rb_clone = Arc::clone(&rb);
            let producer = thread::spawn(move || {
                for i in 0..10000 {
                    while rb_clone.push(black_box(i)).is_err() {
                        thread::yield_now();
                    }
                }
            });

            // Spawn 4 consumers
            for _ in 0..4 {
                let rb_clone = Arc::clone(&rb);
                let handle = thread::spawn(move || {
                    let mut count = 0;
                    while count < 2500 {
                        if rb_clone.pop().is_ok() {
                            count += 1;
                        }
                    }
                });
                handles.push(handle);
            }

            producer.join().unwrap();
            for handle in handles {
                handle.join().unwrap();
            }
        })
    });

    group.finish();
}

fn bench_mpmc_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpmc_concurrent");

    group.bench_function("high_contention", |b| {
        b.iter(|| {
            let rb = Arc::new(MpmcRingBuffer::new(1024));
            let mut handles = vec![];

            // Spawn 4 producers
            for i in 0..4 {
                let rb_clone = Arc::clone(&rb);
                let handle = thread::spawn(move || {
                    for j in 0..2500 {
                        let value = i * 2500 + j;
                        while rb_clone.push(black_box(value)).is_err() {
                            thread::yield_now();
                        }
                    }
                });
                handles.push(handle);
            }

            // Spawn 4 consumers
            for _ in 0..4 {
                let rb_clone = Arc::clone(&rb);
                let handle = thread::spawn(move || {
                    let mut count = 0;
                    while count < 2500 {
                        if rb_clone.pop().is_ok() {
                            count += 1;
                        }
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }
        })
    });

    group.finish();
}

fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("comparison");

    group.bench_function("spsc", |b| {
        b.iter(|| {
            let rb = SpscRingBuffer::new(1024);
            for i in 0..1000 {
                black_box(rb.push(black_box(i))).unwrap();
                black_box(rb.pop()).unwrap();
            }
        })
    });

    group.bench_function("mpsc", |b| {
        b.iter(|| {
            let rb = MpscRingBuffer::new(1024);
            for i in 0..1000 {
                black_box(rb.push(black_box(i))).unwrap();
                black_box(rb.pop()).unwrap();
            }
        })
    });

    group.bench_function("spmc", |b| {
        b.iter(|| {
            let rb = SpmcRingBuffer::new(1024);
            for i in 0..1000 {
                black_box(rb.push(black_box(i))).unwrap();
                black_box(rb.pop()).unwrap();
            }
        })
    });

    group.bench_function("mpmc", |b| {
        b.iter(|| {
            let rb = MpmcRingBuffer::new(1024);
            for i in 0..1000 {
                black_box(rb.push(black_box(i))).unwrap();
                black_box(rb.pop()).unwrap();
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_spsc_single_thread,
    bench_spsc_concurrent,
    bench_mpsc_concurrent,
    bench_spmc_concurrent,
    bench_mpmc_concurrent,
    bench_comparison
);
criterion_main!(benches);

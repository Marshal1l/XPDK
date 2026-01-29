//! Throughput benchmark for XPDK

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use xpdk::{Config, Result, Xpdk};

/// Benchmark packet allocation and deallocation
fn bench_mbuf_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("mbuf_allocation");

    for pool_size in [1024, 4096, 8192].iter() {
        group.bench_with_input(
            BenchmarkId::new("alloc_free", pool_size),
            pool_size,
            |b, &pool_size| {
                let config = Config {
                    pool_size,
                    ..Default::default()
                };

                let xpdk = Xpdk::new(config).unwrap();
                let memory_manager = xpdk.memory_manager();

                b.iter(|| {
                    // Allocate mbuf
                    let mbuf = memory_manager.alloc_mbuf().unwrap();
                    black_box(mbuf);

                    // Free mbuf
                    memory_manager.free_mbuf(mbuf).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark UDP packet processing
fn bench_udp_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("udp_processing");

    for packet_size in [64, 256, 1024, 1400].iter() {
        group.bench_with_input(
            BenchmarkId::new("packet_process", packet_size),
            packet_size,
            |b, &packet_size| {
                let config = Config {
                    pool_size: 4096,
                    ..Default::default()
                };

                let mut xpdk = Xpdk::new(config).unwrap();
                xpdk.start().unwrap();

                let socket_id = {
                    let udp_stack = unsafe { &mut *(xpdk.udp_stack() as *mut _) };
                    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
                    udp_stack.create_socket(local_addr).unwrap()
                };

                let test_data = vec![0u8; packet_size];

                b.iter(|| {
                    // Simulate packet processing
                    let mbuf = xpdk.memory_manager().alloc_mbuf().unwrap();
                    black_box(mbuf);
                    xpdk.memory_manager().free_mbuf(mbuf).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark queue operations
fn bench_queue_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_operations");

    for queue_size in [1024, 4096, 16384].iter() {
        group.bench_with_input(
            BenchmarkId::new("spsc_push_pop", queue_size),
            queue_size,
            |b, &queue_size| {
                use lockfree_ringbuf::SpscRingBuffer;

                let queue = SpscRingBuffer::new(queue_size).unwrap();
                let test_data = vec![0u8; 1024];

                b.iter(|| {
                    // Push
                    for _ in 0..100 {
                        let _ = queue.push(black_box(test_data.clone()));
                    }

                    // Pop
                    for _ in 0..100 {
                        let _ = queue.pop();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark checksum calculation
fn bench_checksum_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("checksum_calculation");

    for data_size in [64, 256, 1024, 1500].iter() {
        group.bench_with_input(
            BenchmarkId::new("ipv4_checksum", data_size),
            data_size,
            |b, &data_size| {
                use xpdk::utils::offload::ChecksumCalculator;

                let calculator = ChecksumCalculator::new(false);
                let test_data = vec![0u8; data_size];

                b.iter(|| {
                    black_box(calculator.ipv4_checksum(black_box(&test_data)).unwrap());
                });
            },
        );
    }

    group.finish();
}

/// Benchmark RSS hash calculation
fn bench_rss_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("rss_hash");

    for data_size in [64, 256, 1024, 1500].iter() {
        group.bench_with_input(
            BenchmarkId::new("toeplitz_hash", data_size),
            data_size,
            |b, &data_size| {
                use xpdk::utils::offload::RssHashCalculator;
                use xpdk::utils::offload::RssHashFunction;

                let calculator = RssHashCalculator::new(RssHashFunction::Toeplitz);
                let test_data = vec![0u8; data_size];

                b.iter(|| {
                    black_box(calculator.calculate(black_box(&test_data)).unwrap());
                });
            },
        );
    }

    group.finish();
}

/// Benchmark memory operations
fn bench_memory_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_operations");

    for data_size in [64, 256, 1024, 4096].iter() {
        group.bench_with_input(
            BenchmarkId::new("memcpy", data_size),
            data_size,
            |b, &data_size| {
                let src = vec![0u8; data_size];
                let mut dst = vec![0u8; data_size];

                b.iter(|| unsafe {
                    std::ptr::copy_nonoverlapping(
                        black_box(src.as_ptr()),
                        black_box(dst.as_mut_ptr()),
                        black_box(data_size),
                    );
                });
            },
        );
    }

    group.finish();
}

/// Benchmark timestamp operations
fn bench_timestamp_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("timestamp_operations");

    group.bench_function("high_res_timer", |b| {
        use xpdk::utils::time::HighResTimer;
        use xpdk::utils::time::TimestampSource;

        let timer = HighResTimer::new(TimestampSource::MonotonicClock);

        b.iter(|| {
            black_box(timer.now());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_mbuf_allocation,
    bench_udp_processing,
    bench_queue_operations,
    bench_checksum_calculation,
    bench_rss_hash,
    bench_memory_operations,
    bench_timestamp_operations
);

criterion_main!(benches);

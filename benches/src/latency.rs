//! Latency benchmark for XPDK

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};
use xpdk::{Config, Result, Xpdk};

/// Benchmark packet allocation latency
fn bench_allocation_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_latency");

    let config = Config::default();
    let xpdk = Xpdk::new(config).unwrap();
    let memory_manager = xpdk.memory_manager();

    group.bench_function("mbuf_alloc", |b| {
        b.iter(|| {
            let mbuf = memory_manager.alloc_mbuf().unwrap();
            black_box(mbuf);
            memory_manager.free_mbuf(mbuf).unwrap();
        });
    });

    group.finish();
}

/// Benchmark UDP socket creation latency
fn bench_socket_creation_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("socket_creation_latency");

    group.bench_function("udp_socket_create", |b| {
        b.iter(|| {
            let config = Config::default();
            let mut xpdk = Xpdk::new(config).unwrap();
            let udp_stack = unsafe { &mut *(xpdk.udp_stack() as *mut _) };
            let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let socket_id = udp_stack.create_socket(local_addr).unwrap();
            black_box(socket_id);
        });
    });

    group.finish();
}

/// Benchmark packet processing latency
fn bench_packet_processing_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_processing_latency");

    for packet_size in [64, 256, 1024, 1400].iter() {
        group.bench_with_input(
            BenchmarkId::new("udp_packet_process", packet_size),
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
                    // Allocate mbuf
                    let mbuf = xpdk.memory_manager().alloc_mbuf().unwrap();
                    black_box(mbuf);
                    xpdk.memory_manager().free_mbuf(mbuf).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark checksum calculation latency
fn bench_checksum_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("checksum_latency");

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

/// Benchmark RSS hash calculation latency
fn bench_rss_hash_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("rss_hash_latency");

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

/// Benchmark queue operation latency
fn bench_queue_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_latency");

    for queue_size in [1024, 4096, 16384].iter() {
        group.bench_with_input(
            BenchmarkId::new("spsc_queue", queue_size),
            queue_size,
            |b, &queue_size| {
                use lockfree_ringbuf::SpscRingBuffer;

                let queue = SpscRingBuffer::new(queue_size).unwrap();
                let test_data = vec![0u8; 1024];

                // Pre-populate queue
                for _ in 0..queue_size / 2 {
                    let _ = queue.push(test_data.clone());
                }

                b.iter(|| {
                    // Push and pop
                    if let Ok(_) = queue.push(black_box(test_data.clone())) {
                        let _ = queue.pop();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark timestamp precision
fn bench_timestamp_precision(c: &mut Criterion) {
    let mut group = c.benchmark_group("timestamp_precision");

    use xpdk::utils::time::HighResTimer;
    use xpdk::utils::time::TimestampSource;

    let timer = HighResTimer::new(TimestampSource::MonotonicClock);

    group.bench_function("high_res_timestamp", |b| {
        b.iter(|| {
            let start = timer.now();
            black_box(start);
            let end = timer.now();
            black_box(end);
        });
    });

    group.finish();
}

/// Benchmark memory copy latency
fn bench_memory_copy_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_copy_latency");

    for data_size in [64, 256, 1024, 4096, 16384].iter() {
        group.bench_with_input(
            BenchmarkId::new("memcpy", data_size),
            data_size,
            |b, &data_size| {
                let src = vec![0u8; data_size];
                let mut dst = vec![0u8; data_size];

                b.iter(|| {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            black_box(src.as_ptr()),
                            black_box(dst.as_mut_ptr()),
                            black_box(data_size),
                        );
                    }
                    black_box(&dst);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark cache prefetch effectiveness
fn bench_cache_prefetch(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_prefetch");

    use xpdk::utils::cpu::CpuPrefetch;

    let data_size = 1024 * 1024; // 1MB
    let data = vec![0u8; data_size];

    group.bench_function("prefetch_l1", |b| {
        b.iter(|| {
            for i in (0..data_size).step_by(64) {
                CpuPrefetch::prefetch_l1(black_box(data.as_ptr().add(i)));
            }
        });
    });

    group.bench_function("prefetch_l2", |b| {
        b.iter(|| {
            for i in (0..data_size).step_by(64) {
                CpuPrefetch::prefetch_l2(black_box(data.as_ptr().add(i)));
            }
        });
    });

    group.finish();
}

criterion_group!(
    latency_benches,
    bench_allocation_latency,
    bench_socket_creation_latency,
    bench_packet_processing_latency,
    bench_checksum_latency,
    bench_rss_hash_latency,
    bench_queue_latency,
    bench_timestamp_precision,
    bench_memory_copy_latency,
    bench_cache_prefetch
);

criterion_main!(latency_benches);

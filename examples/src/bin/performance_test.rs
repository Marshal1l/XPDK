//! Performance test for XPDK UDP stack

use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use xpdk::{Config, Result, UdpStack, Xpdk};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    println!("XPDK Performance Test");
    println!("====================");

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <mode> [options]", args[0]);
        eprintln!("Modes:");
        eprintln!("  server <port> <interface>  - Run as UDP server");
        eprintln!("  client <server_ip> <port> <interface> - Run as UDP client");
        eprintln!("  loopback <port> <interface> - Run loopback test");
        return Ok(());
    }

    let mode = &args[1];

    match mode.as_str() {
        "server" => run_server(&args)?,
        "client" => run_client(&args)?,
        "loopback" => run_loopback(&args)?,
        _ => {
            eprintln!("Unknown mode: {}", mode);
            return Ok(());
        }
    }

    Ok(())
}

/// Run UDP server performance test
fn run_server(args: &[String]) -> Result<()> {
    let port: u16 = if args.len() > 2 {
        args[2].parse().unwrap_or(8080)
    } else {
        8080
    };

    let interface = if args.len() > 3 {
        args[3].clone()
    } else {
        "eth0".to_string()
    };

    println!("Server Mode - Port: {}, Interface: {}", port, interface);

    // Create configuration optimized for performance
    let config = Config {
        interface,
        pool_size: 8192,
        rx_queue_count: 4,
        tx_queue_count: 4,
        rx_queue_size: 4096,
        tx_queue_size: 4096,
        enable_hugepages: true,
        enable_numa: true,
        enable_offload: true,
        ..Default::default()
    };

    // Create and start XPDK
    let mut xpdk = Xpdk::new(config)?;
    xpdk.start()?;

    println!("✓ XPDK started");

    // Create UDP socket
    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
    let socket_id = {
        let udp_stack = xpdk.udp_stack_mut();
        udp_stack.create_socket(local_addr)?
    };

    // Setup signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal...");
        r.store(false, Ordering::Relaxed);
    })
    .unwrap_or_else(|_| {
        eprintln!("Warning: Could not set Ctrl-C handler");
    });

    println!("✓ Server running on {}", local_addr);
    println!("Press Ctrl+C to stop and see results");
    println!();

    // Performance tracking
    let start_time = Instant::now();
    let mut packets_processed = 0u64;
    let mut bytes_received = 0u64;
    let mut bytes_sent = 0u64;
    let mut last_report = start_time;

    // Main processing loop
    while running.load(Ordering::Relaxed) {
        let batch_start = Instant::now();
        let mut batch_processed = 0u32;

        // Process packets in batches
        for _ in 0..64 {
            match process_server_packet(&mut xpdk, socket_id, &mut bytes_received, &mut bytes_sent)
            {
                Ok(processed) => {
                    if processed {
                        batch_processed += 1;
                        packets_processed += 1;
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Report statistics every second
        if last_report.elapsed() >= Duration::from_secs(1) {
            let elapsed = start_time.elapsed();
            let current_pps = if elapsed.as_secs_f64() > 0.0 {
                packets_processed as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            let current_mbps = if elapsed.as_secs_f64() > 0.0 {
                (bytes_received as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            println!("Time: {:6.0}s | Packets: {:10} | PPS: {:8.0} | Throughput: {:8.2} MB/s | Batch: {:3}",
                    elapsed.as_secs(), packets_processed, current_pps, current_mbps, batch_processed);

            last_report = Instant::now();
        }

        // Sleep briefly if no packets processed
        if batch_processed == 0 {
            thread::sleep(Duration::from_micros(10));
        }
    }

    // Print final results
    let total_time = start_time.elapsed();
    print_performance_results(
        "Server",
        packets_processed,
        bytes_received,
        bytes_sent,
        total_time,
    );

    // Cleanup
    xpdk.stop()?;
    Ok(())
}

/// Run UDP client performance test
fn run_client(args: &[String]) -> Result<()> {
    let server_ip: Ipv4Addr = if args.len() > 2 {
        args[2]
            .parse()
            .unwrap_or_else(|_| Ipv4Addr::new(127, 0, 0, 1))
    } else {
        Ipv4Addr::new(127, 0, 0, 1)
    };

    let server_port: u16 = if args.len() > 3 {
        args[3].parse().unwrap_or(8080)
    } else {
        8080
    };

    let interface = if args.len() > 4 {
        args[4].clone()
    } else {
        "eth0".to_string()
    };

    let server_addr = SocketAddr::new(IpAddr::V4(server_ip), server_port);

    println!(
        "Client Mode - Server: {}, Interface: {}",
        server_addr, interface
    );

    // Create configuration optimized for performance
    let config = Config {
        interface,
        pool_size: 8192,
        rx_queue_count: 2,
        tx_queue_count: 2,
        rx_queue_size: 2048,
        tx_queue_size: 2048,
        enable_hugepages: true,
        enable_numa: true,
        enable_offload: true,
        ..Default::default()
    };

    // Create and start XPDK
    let mut xpdk = Xpdk::new(config)?;
    xpdk.start()?;

    println!("✓ XPDK started");

    // Create UDP socket
    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
    let socket_id = {
        let udp_stack = xpdk.udp_stack_mut();
        udp_stack.create_socket(local_addr)?
    };

    // Setup signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal...");
        r.store(false, Ordering::Relaxed);
    })
    .unwrap_or_else(|_| {
        eprintln!("Warning: Could not set Ctrl-C handler");
    });

    println!("✓ Client running from {}", local_addr);
    println!("Press Ctrl+C to stop and see results");
    println!();

    // Performance tracking
    let start_time = Instant::now();
    let mut packets_sent = 0u64;
    let mut packets_received = 0u64;
    let mut bytes_sent = 0u64;
    let mut bytes_received = 0u64;
    let mut last_report = start_time;

    // Test data
    let test_data = vec![0u8; 1024]; // 1KB packets

    // Main processing loop
    while running.load(Ordering::Relaxed) {
        // Send packets in burst
        for _ in 0..32 {
            match send_client_packet(&mut xpdk, socket_id, server_addr, &test_data) {
                Ok(_) => {
                    packets_sent += 1;
                    bytes_sent += test_data.len() as u64;
                }
                Err(_) => break,
            }
        }

        // Receive packets
        for _ in 0..32 {
            match receive_client_packet(&mut xpdk, socket_id, &mut bytes_received) {
                Ok(received) => {
                    if received {
                        packets_received += 1;
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Report statistics every second
        if last_report.elapsed() >= Duration::from_secs(1) {
            let elapsed = start_time.elapsed();
            let pps_tx = if elapsed.as_secs_f64() > 0.0 {
                packets_sent as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            let pps_rx = if elapsed.as_secs_f64() > 0.0 {
                packets_received as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            let mbps_tx = if elapsed.as_secs_f64() > 0.0 {
                (bytes_sent as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            let mbps_rx = if elapsed.as_secs_f64() > 0.0 {
                (bytes_received as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            println!(
                "Time: {:6.0}s | TX: {:8.0} pps ({:6.2} MB/s) | RX: {:8.0} pps ({:6.2} MB/s)",
                elapsed.as_secs(),
                pps_tx,
                mbps_tx,
                pps_rx,
                mbps_rx
            );

            last_report = Instant::now();
        }

        // Brief sleep to prevent overwhelming the system
        thread::sleep(Duration::from_micros(100));
    }

    // Print final results
    let total_time = start_time.elapsed();
    print_performance_results(
        "Client",
        packets_sent,
        bytes_sent,
        bytes_received,
        total_time,
    );

    // Cleanup
    xpdk.stop()?;
    Ok(())
}

/// Run loopback performance test
fn run_loopback(args: &[String]) -> Result<()> {
    let port: u16 = if args.len() > 2 {
        args[2].parse().unwrap_or(8080)
    } else {
        8080
    };

    let interface = if args.len() > 3 {
        args[3].clone()
    } else {
        "eth0".to_string()
    };

    println!("Loopback Mode - Port: {}, Interface: {}", port, interface);

    // Create configuration optimized for performance
    let config = Config {
        interface,
        pool_size: 16384,
        rx_queue_count: 4,
        tx_queue_count: 4,
        rx_queue_size: 8192,
        tx_queue_size: 8192,
        enable_hugepages: true,
        enable_numa: true,
        enable_offload: true,
        ..Default::default()
    };

    // Create and start XPDK
    let mut xpdk = Xpdk::new(config)?;
    xpdk.start()?;

    println!("✓ XPDK started");

    // Create UDP socket
    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    let socket_id = {
        let udp_stack = xpdk.udp_stack_mut();
        udp_stack.create_socket(local_addr)?
    };

    // Setup signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal...");
        r.store(false, Ordering::Relaxed);
    })
    .unwrap_or_else(|_| {
        eprintln!("Warning: Could not set Ctrl-C handler");
    });

    println!("✓ Loopback test running on {}", local_addr);
    println!("Press Ctrl+C to stop and see results");
    println!();

    // Performance tracking
    let start_time = Instant::now();
    let mut packets_sent = 0u64;
    let mut packets_received = 0u64;
    let mut bytes_sent = 0u64;
    let mut bytes_received = 0u64;
    let mut last_report = start_time;

    // Test data
    let test_data = vec![0u8; 1400]; // Near MTU size

    // Main processing loop
    while running.load(Ordering::Relaxed) {
        // Send packet to self
        match send_client_packet(&mut xpdk, socket_id, local_addr, &test_data) {
            Ok(_) => {
                packets_sent += 1;
                bytes_sent += test_data.len() as u64;
            }
            Err(_) => {}
        }

        // Receive the packet
        match receive_client_packet(&mut xpdk, socket_id, &mut bytes_received) {
            Ok(received) => {
                if received {
                    packets_received += 1;
                }
            }
            Err(_) => {}
        }

        // Report statistics every second
        if last_report.elapsed() >= Duration::from_secs(1) {
            let elapsed = start_time.elapsed();
            let pps = if elapsed.as_secs_f64() > 0.0 {
                packets_sent as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };

            let mbps = if elapsed.as_secs_f64() > 0.0 {
                (bytes_sent as f64 / elapsed.as_secs_f64()) / (1024.0 * 1024.0)
            } else {
                0.0
            };

            let loss_rate = if packets_sent > 0 {
                (packets_sent - packets_received) as f64 / packets_sent as f64 * 100.0
            } else {
                0.0
            };

            println!(
                "Time: {:6.0}s | PPS: {:8.0} | Throughput: {:8.2} MB/s | Loss: {:5.2}%",
                elapsed.as_secs(),
                pps,
                mbps,
                loss_rate
            );

            last_report = Instant::now();
        }
    }

    // Print final results
    let total_time = start_time.elapsed();
    print_performance_results(
        "Loopback",
        packets_sent,
        bytes_sent,
        bytes_received,
        total_time,
    );

    // Cleanup
    xpdk.stop()?;
    Ok(())
}

/// Process a server packet
fn process_server_packet(
    xpdk: &mut Xpdk,
    socket_id: u16,
    bytes_received: &mut u64,
    bytes_sent: &mut u64,
) -> Result<bool> {
    let recv_result = {
        let udp_stack = xpdk.udp_stack_mut();
        let socket = match udp_stack.get_socket_mut(socket_id) {
            Some(socket) => socket,
            None => return Ok(false),
        };
        socket.recv()
    };

    match recv_result {
        Ok(packet) => {
            let payload = packet.payload();
            let src_addr = packet.src_addr();
            let mbuf = packet.mbuf;

            *bytes_received += payload.len() as u64;

            // Echo back
            let send_result = {
                let udp_stack = xpdk.udp_stack_mut();
                let socket = udp_stack.get_socket_mut(socket_id).unwrap();
                socket.send(src_addr, payload)
            };

            match send_result {
                Ok(_) => {
                    *bytes_sent += payload.len() as u64;
                }
                Err(_) => {}
            }

            // Free the packet mbuf
            let _ = xpdk.memory_manager().free_mbuf(mbuf);
            Ok(true)
        }
        Err(xpdk::Error::NetworkError(_)) => Ok(false),
        Err(_) => Ok(false),
    }
}

/// Send a client packet
fn send_client_packet(
    xpdk: &mut Xpdk,
    socket_id: u16,
    server_addr: SocketAddr,
    data: &[u8],
) -> Result<()> {
    let udp_stack = xpdk.udp_stack_mut();
    let socket = match udp_stack.get_socket_mut(socket_id) {
        Some(socket) => socket,
        None => return Err(xpdk::Error::NetworkError("Socket not found".to_string())),
    };

    socket.send(server_addr, data)
}

/// Receive a client packet
fn receive_client_packet(
    xpdk: &mut Xpdk,
    socket_id: u16,
    bytes_received: &mut u64,
) -> Result<bool> {
    let recv_result = {
        let udp_stack = xpdk.udp_stack_mut();
        let socket = match udp_stack.get_socket_mut(socket_id) {
            Some(socket) => socket,
            None => return Ok(false),
        };
        socket.recv()
    };

    match recv_result {
        Ok(packet) => {
            let payload = packet.payload();
            *bytes_received += payload.len() as u64;

            // Free the packet mbuf
            let _ = xpdk.memory_manager().free_mbuf(packet.mbuf);
            Ok(true)
        }
        Err(xpdk::Error::NetworkError(_)) => Ok(false),
        Err(_) => Ok(false),
    }
}

/// Print performance results
fn print_performance_results(
    mode: &str,
    packets: u64,
    bytes_tx: u64,
    bytes_rx: u64,
    elapsed: Duration,
) {
    println!("\n{} Performance Results", mode);
    println!("========================");

    let elapsed_secs = elapsed.as_secs_f64();

    if elapsed_secs > 0.0 {
        let pps = packets as f64 / elapsed_secs;
        let mbps_tx = (bytes_tx as f64 / elapsed_secs) / (1024.0 * 1024.0);
        let mbps_rx = (bytes_rx as f64 / elapsed_secs) / (1024.0 * 1024.0);

        println!("Total Time:     {:.3} seconds", elapsed_secs);
        println!("Total Packets:  {}", packets);
        println!("Packets/sec:    {:.0}", pps);
        println!("Throughput TX:  {:.2} MB/s", mbps_tx);
        println!("Throughput RX:  {:.2} MB/s", mbps_rx);

        if bytes_tx > 0 {
            println!(
                "Average Packet Size: {:.1} bytes",
                bytes_tx as f64 / packets as f64
            );
        }
    }

    println!("========================");
}

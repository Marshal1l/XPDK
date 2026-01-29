//! UDP echo server example using XPDK

use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use xpdk::{Config, Result, UdpStack, Xpdk};

fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

    println!("XPDK UDP Echo Server");
    println!("====================");

    // Create configuration
    let mut config = Config {
        interface: "eth0".to_string(), // Change this to your network interface
        pool_size: 4096,
        rx_queue_count: 2,
        tx_queue_count: 2,
        rx_queue_size: 1024,
        tx_queue_size: 1024,
        enable_hugepages: true,
        enable_numa: true,
        enable_offload: true,
        ..Default::default()
    };

    // Override with command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        config.interface = args[1].clone();
    }

    let port = if args.len() > 2 {
        args[2].parse().unwrap_or(8080)
    } else {
        8080
    };

    println!("Using interface: {}", config.interface);
    println!("Listening on port: {}", port);

    // Save interface name before moving config
    let interface_name = config.interface.clone();

    // Create XPDK instance
    let mut xpdk = match Xpdk::new(config) {
        Ok(xpdk) => {
            println!("✓ XPDK initialized successfully");
            xpdk
        }
        Err(e) => {
            eprintln!("✗ Failed to initialize XPDK: {}", e);
            eprintln!("Make sure:");
            eprintln!("  1. Network interface '{}' exists", interface_name);
            eprintln!("  2. You have root privileges (required for libpcap)");
            eprintln!("  3. libpcap development libraries are installed");
            return Ok(());
        }
    };

    // Start XPDK
    if let Err(e) = xpdk.start() {
        eprintln!("✗ Failed to start XPDK: {}", e);
        return Ok(());
    }

    println!("✓ XPDK started");

    // Create UDP socket
    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
    let socket_id = {
        let udp_stack = xpdk.udp_stack_mut();
        match udp_stack.create_socket(local_addr) {
            Ok(id) => {
                println!("✓ UDP socket created on {}", local_addr);
                id
            }
            Err(e) => {
                eprintln!("✗ Failed to create UDP socket: {}", e);
                return Ok(());
            }
        }
    };

    // Setup signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nReceived shutdown signal...");
        r.store(false, Ordering::Relaxed);
    })
    .unwrap_or_else(|_| {
        eprintln!("Warning: Could not set Ctrl-C handler");
    });

    println!("✓ Echo server is running...");
    println!("Press Ctrl+C to stop");
    println!();

    // Main processing loop
    let mut packets_processed = 0u64;
    let mut bytes_received = 0u64;
    let mut bytes_sent = 0u64;

    let start_time = std::time::Instant::now();

    while running.load(Ordering::Relaxed) {
        // Process packets
        match process_packets(
            &mut xpdk,
            socket_id,
            &mut packets_processed,
            &mut bytes_received,
            &mut bytes_sent,
        ) {
            Ok(processed) => {
                if processed == 0 {
                    // No packets, sleep briefly
                    thread::sleep(Duration::from_millis(1));
                }
            }
            Err(e) => {
                eprintln!("Error processing packets: {}", e);
                thread::sleep(Duration::from_millis(10));
            }
        }

        // Print statistics every 10 seconds
        if start_time.elapsed().as_secs() % 10 == 0 && start_time.elapsed().as_secs() > 0 {
            print_statistics(
                packets_processed,
                bytes_received,
                bytes_sent,
                start_time.elapsed(),
            );
        }
    }

    // Shutdown
    println!("\nShutting down...");

    if let Err(e) = xpdk.stop() {
        eprintln!("✗ Error stopping XPDK: {}", e);
    } else {
        println!("✓ XPDK stopped");
    }

    // Print final statistics
    let total_time = start_time.elapsed();
    print_statistics(packets_processed, bytes_received, bytes_sent, total_time);

    println!("✓ Server shutdown complete");
    Ok(())
}

/// Process incoming packets and send echo responses
fn process_packets(
    xpdk: &mut Xpdk,
    socket_id: u16,
    packets_processed: &mut u64,
    bytes_received: &mut u64,
    bytes_sent: &mut u64,
) -> Result<u32> {
    let mut processed = 0u32;

    // Process up to 32 packets per batch
    for _ in 0..32 {
        // Get UDP socket for receiving
        let recv_result = {
            let udp_stack = xpdk.udp_stack_mut();
            let socket = match udp_stack.get_socket_mut(socket_id) {
                Some(socket) => socket,
                None => return Ok(0),
            };
            socket.recv()
        };

        match recv_result {
            Ok(packet) => {
                let payload = packet.payload();
                let src_addr = packet.src_addr();
                let dst_addr = packet.dst_addr();
                let mbuf = packet.mbuf;

                // Update statistics
                *bytes_received += payload.len() as u64;

                // Echo the packet back
                let send_result = {
                    let udp_stack = xpdk.udp_stack_mut();
                    let socket = udp_stack.get_socket_mut(socket_id).unwrap();
                    socket.send(src_addr, payload)
                };

                match send_result {
                    Ok(_) => {
                        *bytes_sent += payload.len() as u64;
                        *packets_processed += 1;
                        processed += 1;

                        // Print first few packets for debugging
                        if *packets_processed <= 5 {
                            println!(
                                "Echo: {} -> {} ({} bytes)",
                                src_addr,
                                dst_addr,
                                payload.len()
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to send echo: {}", e);
                    }
                }

                // Free the packet mbuf
                if let Err(e) = xpdk.memory_manager().free_mbuf(mbuf) {
                    eprintln!("Failed to free mbuf: {}", e);
                }
            }
            Err(xpdk::Error::NetworkError(_)) => {
                // No packets available
                break;
            }
            Err(e) => {
                eprintln!("Error receiving packet: {}", e);
                break;
            }
        }
    }

    Ok(processed)
}

/// Print server statistics
fn print_statistics(packets: u64, bytes_rx: u64, bytes_tx: u64, elapsed: Duration) {
    let elapsed_secs = elapsed.as_secs_f64();
    let packets_per_sec = if elapsed_secs > 0.0 {
        packets as f64 / elapsed_secs
    } else {
        0.0
    };
    let mbps_rx = if elapsed_secs > 0.0 {
        (bytes_rx as f64 / elapsed_secs) / (1024.0 * 1024.0)
    } else {
        0.0
    };
    let mbps_tx = if elapsed_secs > 0.0 {
        (bytes_tx as f64 / elapsed_secs) / (1024.0 * 1024.0)
    } else {
        0.0
    };

    print!(
        "\rPackets: {:10} | RX: {:8.2} MB/s | TX: {:8.2} MB/s | PPS: {:8.0} | Time: {:8.0}s",
        packets, mbps_rx, mbps_tx, packets_per_sec, elapsed_secs
    );
    io::stdout().flush().unwrap();
}

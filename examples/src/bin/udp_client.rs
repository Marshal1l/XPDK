//! UDP client example using XPDK

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

    println!("XPDK UDP Client");
    println!("================");

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!(
            "Usage: {} <server_ip> <server_port> [local_port] [interface]",
            args[0]
        );
        eprintln!("Example: {} 192.168.1.100 8080 0 eth0", args[0]);
        return Ok(());
    }

    let server_ip: Ipv4Addr = args[1]
        .parse()
        .map_err(|_| xpdk::Error::InvalidConfig("Invalid server IP address".to_string()))?;
    let server_port: u16 = args[2]
        .parse()
        .map_err(|_| xpdk::Error::InvalidConfig("Invalid server port".to_string()))?;
    let local_port: u16 = if args.len() > 3 {
        args[3].parse().unwrap_or(0)
    } else {
        0 // Let OS choose
    };
    let interface = if args.len() > 4 {
        args[4].clone()
    } else {
        "eth0".to_string()
    };

    let server_addr = SocketAddr::new(IpAddr::V4(server_ip), server_port);
    let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), local_port);

    println!("Server: {}", server_addr);
    println!("Local:  {}", local_addr);
    println!("Interface: {}", interface);

    // Save interface name before moving
    let interface_name = interface.clone();

    // Create configuration
    let config = Config {
        interface,
        pool_size: 2048,
        rx_queue_count: 1,
        tx_queue_count: 1,
        rx_queue_size: 512,
        tx_queue_size: 512,
        enable_hugepages: true,
        enable_numa: true,
        enable_offload: true,
        ..Default::default()
    };

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

    println!("✓ Client is running...");
    println!("Press Ctrl+C to stop");
    println!();

    // Main processing loop
    let mut packets_sent = 0u64;
    let mut packets_received = 0u64;
    let mut bytes_sent = 0u64;
    let mut bytes_received = 0u64;

    let start_time = std::time::Instant::now();
    let mut last_send_time = start_time;

    // Test data
    let test_message = b"Hello from XPDK UDP client!";
    let mut counter = 0u32;

    while running.load(Ordering::Relaxed) {
        // Send a packet every 100ms
        if last_send_time.elapsed() >= Duration::from_millis(100) {
            let message = format!(
                "{}: {}",
                std::str::from_utf8(test_message).unwrap(),
                counter
            );
            let message_bytes = message.as_bytes();

            match send_packet(&mut xpdk, socket_id, server_addr, message_bytes) {
                Ok(_) => {
                    packets_sent += 1;
                    bytes_sent += message_bytes.len() as u64;
                    counter += 1;
                    last_send_time = std::time::Instant::now();

                    // Print first few packets for debugging
                    if packets_sent <= 5 {
                        println!(
                            "Sent: {} -> {} ({} bytes)",
                            local_addr,
                            server_addr,
                            message_bytes.len()
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Failed to send packet: {}", e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        // Receive packets
        match receive_packets(
            &mut xpdk,
            socket_id,
            &mut packets_received,
            &mut bytes_received,
        ) {
            Ok(received) => {
                if received == 0 {
                    // No packets, sleep briefly
                    thread::sleep(Duration::from_millis(1));
                }
            }
            Err(e) => {
                eprintln!("Error receiving packets: {}", e);
                thread::sleep(Duration::from_millis(10));
            }
        }

        // Print statistics every 5 seconds
        if start_time.elapsed().as_secs() % 5 == 0 && start_time.elapsed().as_secs() > 0 {
            print_client_statistics(
                packets_sent,
                packets_received,
                bytes_sent,
                bytes_received,
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
    print_client_statistics(
        packets_sent,
        packets_received,
        bytes_sent,
        bytes_received,
        total_time,
    );

    println!("✓ Client shutdown complete");
    Ok(())
}

/// Send a packet to the server
fn send_packet(
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

/// Receive packets from the server
fn receive_packets(
    xpdk: &mut Xpdk,
    socket_id: u16,
    packets_received: &mut u64,
    bytes_received: &mut u64,
) -> Result<u32> {
    let mut received = 0u32;

    // Process up to 16 packets per batch
    for _ in 0..16 {
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

                // Update statistics
                *bytes_received += payload.len() as u64;
                *packets_received += 1;
                received += 1;

                // Print first few packets for debugging
                if *packets_received <= 5 {
                    println!(
                        "Received: {} -> {} ({} bytes): {}",
                        src_addr,
                        packet.dst_addr(),
                        payload.len(),
                        String::from_utf8_lossy(payload)
                    );
                }

                // Free the packet mbuf
                if let Err(e) = xpdk.memory_manager().free_mbuf(packet.mbuf) {
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

    Ok(received)
}

/// Print client statistics
fn print_client_statistics(
    packets_sent: u64,
    packets_received: u64,
    bytes_sent: u64,
    bytes_received: u64,
    elapsed: Duration,
) {
    let elapsed_secs = elapsed.as_secs_f64();
    let pps_tx = if elapsed_secs > 0.0 {
        packets_sent as f64 / elapsed_secs
    } else {
        0.0
    };
    let pps_rx = if elapsed_secs > 0.0 {
        packets_received as f64 / elapsed_secs
    } else {
        0.0
    };
    let mbps_tx = if elapsed_secs > 0.0 {
        (bytes_sent as f64 / elapsed_secs) / (1024.0 * 1024.0)
    } else {
        0.0
    };
    let mbps_rx = if elapsed_secs > 0.0 {
        (bytes_received as f64 / elapsed_secs) / (1024.0 * 1024.0)
    } else {
        0.0
    };

    print!("\rSent: {:8} ({:6.0} pps, {:6.2} MB/s) | Received: {:8} ({:6.0} pps, {:6.2} MB/s) | Time: {:6.0}s",
           packets_sent, pps_tx, mbps_tx,
           packets_received, pps_rx, mbps_rx,
           elapsed_secs);
    io::stdout().flush().unwrap();
}

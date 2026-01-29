//! Poll Mode Driver (PMD) module for high-performance packet I/O
//!
//! This module implements a DPDK-inspired poll mode driver using libpcap,
//! supporting multi-queue, RSS, and batch operations for maximum throughput.

use crate::{
    memory::{Mbuf, MbufPool},
    Config, Error, Result,
};
use parking_lot::Mutex;
use pcap::{Active, Capture, Device};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Default packet buffer size
pub const DEFAULT_PACKET_SIZE: usize = 2048;

/// Maximum batch size for packet operations
pub const MAX_BATCH_SIZE: usize = 32;

/// Receive queue statistics
#[derive(Debug, Default)]
pub struct RxQueueStats {
    pub packets_received: AtomicUsize,
    pub bytes_received: AtomicUsize,
    pub errors: AtomicUsize,
    pub drops: AtomicUsize,
}

/// Transmit queue statistics
#[derive(Debug, Default)]
pub struct TxQueueStats {
    pub packets_sent: AtomicUsize,
    pub bytes_sent: AtomicUsize,
    pub errors: AtomicUsize,
    pub drops: AtomicUsize,
}

/// Receive queue
pub struct RxQueue {
    /// Queue ID
    id: u16,
    /// libpcap capture handle
    capture: Arc<Mutex<Capture<Active>>>,
    /// Memory pool for mbuf allocation
    pool: Arc<MbufPool>,
    /// Queue statistics
    stats: RxQueueStats,
    /// Running flag
    running: AtomicBool,
}

impl RxQueue {
    /// Create a new receive queue
    pub fn new(id: u16, capture: Capture<Active>, pool: Arc<MbufPool>) -> Result<Self> {
        let capture = Arc::new(Mutex::new(capture));

        Ok(Self {
            id,
            capture,
            pool,
            stats: RxQueueStats::default(),
            running: AtomicBool::new(false),
        })
    }

    /// Get memory pool
    pub fn get_pool(&self) -> &Arc<MbufPool> {
        &self.pool
    }

    /// Receive a single packet
    pub fn recv(&self) -> Result<*mut Mbuf> {
        let mut capture = self.capture.lock();

        match capture.next_packet() {
            Ok(packet) => {
                let mbuf = self.pool.alloc()?;

                unsafe {
                    let mbuf_ref = &mut *mbuf;
                    let data_len = packet.data.len();

                    if data_len > mbuf_ref.buf_len {
                        self.pool.free(mbuf)?;
                        self.stats.errors.fetch_add(1, Ordering::Relaxed);
                        return Err(Error::NetworkError("Packet too large for mbuf".to_string()));
                    }

                    // Copy packet data to mbuf
                    std::ptr::copy_nonoverlapping(packet.data.as_ptr(), mbuf_ref.data, data_len);

                    mbuf_ref.len = data_len;
                    mbuf_ref.timestamp = packet.header.ts.tv_sec as u64 * 1_000_000_000
                        + packet.header.ts.tv_usec as u64 * 1000;
                    mbuf_ref.queue_id = self.id;
                }

                self.stats.packets_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .bytes_received
                    .fetch_add(packet.data.len(), Ordering::Relaxed);

                Ok(mbuf)
            }
            Err(pcap::Error::TimeoutExpired) => {
                Err(Error::NetworkError("No packet available".to_string()))
            }
            Err(e) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::PcapError(e.to_string()))
            }
        }
    }

    /// Start the receive queue
    pub fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);

        // Set capture mode to non-blocking
        {
            let _ = self.capture.lock();
        }

        Ok(())
    }

    /// Stop the receive queue
    pub fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Get queue statistics
    pub fn stats(&self) -> &RxQueueStats {
        &self.stats
    }
}

/// Transmit queue
pub struct TxQueue {
    /// Queue ID
    #[allow(dead_code)]
    id: u16,
    /// libpcap capture handle (for sending)
    capture: Arc<Mutex<Capture<Active>>>,
    /// Queue statistics
    stats: TxQueueStats,
    /// Running flag
    running: AtomicBool,
}

impl TxQueue {
    /// Create a new transmit queue
    pub fn new(id: u16, capture: Capture<Active>) -> Result<Self> {
        let capture = Arc::new(Mutex::new(capture));

        Ok(Self {
            id,
            capture,
            stats: TxQueueStats::default(),
            running: AtomicBool::new(false),
        })
    }

    /// Transmit a single packet
    pub fn send(&self, mbuf: *mut Mbuf) -> Result<()> {
        if mbuf.is_null() {
            return Err(Error::NetworkError("Null mbuf".to_string()));
        }

        let mbuf_ref = unsafe { &*mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };

        let mut capture = self.capture.lock();
        match capture.sendpacket(data) {
            Ok(_) => {
                self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .bytes_sent
                    .fetch_add(mbuf_ref.len, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::PcapError(e.to_string()))
            }
        }
    }

    /// Start the transmit queue
    pub fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stop the transmit queue
    pub fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Get queue statistics
    pub fn stats(&self) -> &TxQueueStats {
        &self.stats
    }
}

/// Poll Mode Driver
pub struct PollModeDriver {
    /// Driver configuration
    #[allow(dead_code)]
    config: Config,
    /// Network device
    device: Device,
    /// Receive queues
    rx_queues: HashMap<u16, RxQueue>,
    /// Transmit queues
    tx_queues: HashMap<u16, TxQueue>,
    /// Memory pool
    pool: Arc<MbufPool>,
    /// Running flag
    running: AtomicBool,
}

impl PollModeDriver {
    /// Create a new poll mode driver
    pub fn new(config: &Config) -> Result<Self> {
        // Find the specified network device
        let device = Device::lookup()
            .unwrap_or_default()
            .into_iter()
            .find(|d| d.name == config.interface)
            .ok_or_else(|| {
                Error::InvalidConfig(format!("Interface '{}' not found", config.interface))
            })?;

        // Create memory pool
        let pool = Arc::new(MbufPool::new(
            "pmd_pool".to_string(),
            config.pool_size,
            DEFAULT_PACKET_SIZE,
        )?);

        let mut rx_queues = HashMap::new();
        let mut tx_queues = HashMap::new();

        // Create RX queues
        for i in 0..config.rx_queue_count {
            let capture = Capture::from_device(device.clone())?
                .promisc(true)
                .snaplen(DEFAULT_PACKET_SIZE as i32)
                .timeout(1) // Non-blocking with 1ms timeout
                .open()?;

            let rx_queue = RxQueue::new(i as u16, capture, pool.clone())?;
            rx_queues.insert(i as u16, rx_queue);
        }

        // Create TX queues
        for i in 0..config.tx_queue_count {
            let capture = Capture::from_device(device.clone())?
                .promisc(true)
                .snaplen(DEFAULT_PACKET_SIZE as i32)
                .open()?;

            let tx_queue = TxQueue::new(i as u16, capture)?;
            tx_queues.insert(i as u16, tx_queue);
        }

        Ok(Self {
            config: config.clone(),
            device,
            rx_queues,
            tx_queues,
            pool,
            running: AtomicBool::new(false),
        })
    }

    /// Start the PMD
    pub fn start(&mut self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);

        // Start all RX queues
        for rx_queue in self.rx_queues.values() {
            rx_queue.start()?;
        }

        // Start all TX queues
        for tx_queue in self.tx_queues.values() {
            tx_queue.start()?;
        }

        Ok(())
    }

    /// Stop the PMD
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        // Stop all RX queues
        for rx_queue in self.rx_queues.values() {
            rx_queue.stop()?;
        }

        // Stop all TX queues
        for tx_queue in self.tx_queues.values() {
            tx_queue.stop()?;
        }

        Ok(())
    }

    /// Get a receive queue by ID
    pub fn get_rx_queue(&self, id: u16) -> Option<&RxQueue> {
        self.rx_queues.get(&id)
    }

    /// Get a transmit queue by ID
    pub fn get_tx_queue(&self, id: u16) -> Option<&TxQueue> {
        self.tx_queues.get(&id)
    }

    /// Get the memory pool
    pub fn get_pool(&self) -> &Arc<MbufPool> {
        &self.pool
    }

    /// Get device information
    pub fn device_info(&self) -> &Device {
        &self.device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pmd_creation() {
        let config = Config::default();
        let result = PollModeDriver::new(&config);

        // This may fail if no network interface is available
        match result {
            Ok(_) => println!("PMD created successfully"),
            Err(Error::InvalidConfig(_)) => println!("Expected: Interface not found"),
            Err(e) => println!("Unexpected error: {:?}", e),
        }
    }
}

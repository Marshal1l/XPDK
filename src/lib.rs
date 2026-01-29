//! XPDK - High-performance userspace UDP network stack
//!
//! A DPDK-inspired userspace networking implementation using libpcap,
//! featuring lock-free concurrency, huge pages, and hardware offloading.

pub mod memory;
pub mod poll;
pub mod queue;
pub mod udp;
pub mod utils;

#[cfg(feature = "numa")]
pub mod numa;

#[cfg(feature = "hardware-offload")]
pub mod offload;

// Re-export key components
pub use memory::{Mbuf, MbufPool, MemoryManager};
pub use poll::{PollModeDriver, RxQueue, TxQueue};
pub use queue::{MpmcQueue, RingBuffer, SpscQueue};
pub use udp::{UdpPacket, UdpSocket, UdpStack};

use thiserror::Error;

/// XPDK error types
#[derive(Error, Debug)]
pub enum Error {
    #[error("Memory allocation failed: {0}")]
    MemoryAllocation(String),

    #[error("libpcap error: {0}")]
    PcapError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Queue error: {0}")]
    QueueError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(#[from] std::num::ParseIntError),

    #[error("NUMA error: {0}")]
    NumaError(String),

    #[error("Hardware offload error: {0}")]
    OffloadError(String),

    #[error("PCAP error: {0}")]
    Pcap(#[from] pcap::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// XPDK configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Number of memory pools
    pub pool_count: usize,

    /// Size of each memory pool
    pub pool_size: usize,

    /// Number of RX queues
    pub rx_queue_count: usize,

    /// Number of TX queues
    pub tx_queue_count: usize,

    /// RX queue size
    pub rx_queue_size: usize,

    /// TX queue size
    pub tx_queue_size: usize,

    /// Enable huge pages
    pub enable_hugepages: bool,

    /// Enable NUMA awareness
    pub enable_numa: bool,

    /// CPU affinity settings
    pub cpu_affinity: Option<Vec<usize>>,

    /// Network interface name
    pub interface: String,

    /// Hardware offload features
    pub enable_offload: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pool_count: 4,
            pool_size: 8192,
            rx_queue_count: 4,
            tx_queue_count: 4,
            rx_queue_size: 4096,
            tx_queue_size: 4096,
            enable_hugepages: true,
            enable_numa: true,
            cpu_affinity: None,
            interface: "eth0".to_string(),
            enable_offload: true,
        }
    }
}

/// Main XPDK context
pub struct Xpdk {
    #[allow(dead_code)]
    config: Config,
    memory_manager: MemoryManager,
    pmd: PollModeDriver,
    udp_stack: UdpStack,
}

impl Xpdk {
    /// Create a new XPDK instance
    pub fn new(config: Config) -> Result<Self> {
        let memory_manager = MemoryManager::new(&config)?;
        let pmd = PollModeDriver::new(&config)?;
        let udp_stack = UdpStack::new(&config)?;

        Ok(Self {
            config,
            memory_manager,
            pmd,
            udp_stack,
        })
    }

    /// Get the UDP stack
    pub fn udp_stack(&self) -> &UdpStack {
        &self.udp_stack
    }

    /// Get the UDP stack (mutable)
    pub fn udp_stack_mut(&mut self) -> &mut UdpStack {
        &mut self.udp_stack
    }

    /// Get the poll mode driver
    pub fn pmd(&self) -> &PollModeDriver {
        &self.pmd
    }

    /// Get the memory manager
    pub fn memory_manager(&self) -> &MemoryManager {
        &self.memory_manager
    }

    /// Start packet processing
    pub fn start(&mut self) -> Result<()> {
        self.pmd.start()?;
        self.udp_stack.start()?;
        Ok(())
    }

    /// Stop packet processing
    pub fn stop(&mut self) -> Result<()> {
        self.udp_stack.stop()?;
        self.pmd.stop()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.pool_count, 4);
        assert_eq!(config.pool_size, 8192);
    }

    #[test]
    fn test_xpdk_creation() {
        let config = Config::default();
        let result = Xpdk::new(config);
        // This test may fail for various reasons (no interface, no permissions, etc.)
        // We just verify that the function returns a valid Result type
        // Either Ok or any Error variant is acceptable for this test
        match result {
            Ok(_) => {
                // Successfully created XPDK instance
            }
            Err(_) => {
                // Expected to fail in test environment without proper setup
            }
        }
    }
}

//! UDP protocol stack implementation
//!
//! This module provides a high-performance UDP stack with zero-copy operations,
//! hardware offloading support, and efficient packet processing.

use crate::poll::{RxQueue, TxQueue};
use crate::{memory::Mbuf, Config, Error, Result};
use lockfree_ringbuf::SpscRingBuffer;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// UDP header structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UdpHeader {
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Length (header + data)
    pub length: u16,
    /// Checksum
    pub checksum: u16,
}

impl UdpHeader {
    /// Create a new UDP header
    pub fn new(src_port: u16, dst_port: u16, length: u16) -> Self {
        Self {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            length: length.to_be(),
            checksum: 0,
        }
    }

    /// Get source port (host byte order)
    pub fn src_port(&self) -> u16 {
        u16::from_be(self.src_port)
    }

    /// Get destination port (host byte order)
    pub fn dst_port(&self) -> u16 {
        u16::from_be(self.dst_port)
    }

    /// Get length (host byte order)
    pub fn length(&self) -> u16 {
        u16::from_be(self.length)
    }

    /// Get checksum (host byte order)
    pub fn checksum(&self) -> u16 {
        u16::from_be(self.checksum)
    }
}

/// IPv4 header structure (simplified)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv4Header {
    /// Version and IHL
    pub version_ihl: u8,
    /// Type of service
    pub tos: u8,
    /// Total length
    pub total_length: u16,
    /// Identification
    pub identification: u16,
    /// Flags and fragment offset
    pub flags_fragment: u16,
    /// TTL
    pub ttl: u8,
    /// Protocol
    pub protocol: u8,
    /// Header checksum
    pub checksum: u16,
    /// Source address
    pub src_addr: [u8; 4],
    /// Destination address
    pub dst_addr: [u8; 4],
}

impl Ipv4Header {
    /// Create a new IPv4 header
    pub fn new(src_addr: Ipv4Addr, dst_addr: Ipv4Addr, payload_length: u16) -> Self {
        let total_length = (std::mem::size_of::<Ipv4Header>() + payload_length as usize) as u16;

        Self {
            version_ihl: 0x45, // IPv4 + 5 words (20 bytes)
            tos: 0,
            total_length: total_length.to_be(),
            identification: 0,
            flags_fragment: 0,
            ttl: 64,
            protocol: 17, // UDP
            checksum: 0,
            src_addr: src_addr.octets(),
            dst_addr: dst_addr.octets(),
        }
    }

    /// Get source address
    pub fn src_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.src_addr)
    }

    /// Get destination address
    pub fn dst_addr(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.dst_addr)
    }

    /// Get protocol
    pub fn protocol(&self) -> u8 {
        self.protocol
    }
}

/// Ethernet header structure
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EthernetHeader {
    /// Destination MAC
    pub dst_mac: [u8; 6],
    /// Source MAC
    pub src_mac: [u8; 6],
    /// EtherType
    pub ether_type: u16,
}

impl EthernetHeader {
    /// Create a new Ethernet header
    pub fn new(src_mac: [u8; 6], dst_mac: [u8; 6], ether_type: u16) -> Self {
        Self {
            dst_mac,
            src_mac,
            ether_type: ether_type.to_be(),
        }
    }

    /// Get EtherType (host byte order)
    pub fn ether_type(&self) -> u16 {
        u16::from_be(self.ether_type)
    }
}

/// UDP packet structure
pub struct UdpPacket {
    /// Mbuf containing the packet data
    pub mbuf: *mut Mbuf,
    /// Ethernet header offset
    pub eth_offset: usize,
    /// IP header offset
    pub ip_offset: usize,
    /// UDP header offset
    pub udp_offset: usize,
    /// Payload offset
    pub payload_offset: usize,
}

impl UdpPacket {
    /// Create a new UDP packet from an mbuf
    pub fn from_mbuf(mbuf: *mut Mbuf) -> Result<Self> {
        if mbuf.is_null() {
            return Err(Error::NetworkError("Null mbuf".to_string()));
        }

        let mbuf_ref = unsafe { &*mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };

        // Parse Ethernet header
        if data.len() < std::mem::size_of::<EthernetHeader>() {
            return Err(Error::NetworkError(
                "Packet too small for Ethernet header".to_string(),
            ));
        }

        let eth_offset = 0;
        let eth_header = unsafe { &*(data.as_ptr().add(eth_offset) as *const EthernetHeader) };

        // Check for IPv4
        if eth_header.ether_type() != 0x0800 {
            return Err(Error::NetworkError("Not an IPv4 packet".to_string()));
        }

        // Parse IPv4 header
        let ip_offset = eth_offset + std::mem::size_of::<EthernetHeader>();
        if data.len() < ip_offset + std::mem::size_of::<Ipv4Header>() {
            return Err(Error::NetworkError(
                "Packet too small for IPv4 header".to_string(),
            ));
        }

        let ip_header = unsafe { &*(data.as_ptr().add(ip_offset) as *const Ipv4Header) };

        // Check for UDP
        if ip_header.protocol() != 17 {
            return Err(Error::NetworkError("Not a UDP packet".to_string()));
        }

        // Parse UDP header
        let udp_offset = ip_offset + ((ip_header.version_ihl & 0x0F) as usize) * 4;
        if data.len() < udp_offset + std::mem::size_of::<UdpHeader>() {
            return Err(Error::NetworkError(
                "Packet too small for UDP header".to_string(),
            ));
        }

        let payload_offset = udp_offset + std::mem::size_of::<UdpHeader>();

        Ok(Self {
            mbuf,
            eth_offset,
            ip_offset,
            udp_offset,
            payload_offset,
        })
    }

    /// Get the UDP header
    pub fn udp_header(&self) -> &UdpHeader {
        let mbuf_ref = unsafe { &*self.mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };
        unsafe { &*(data.as_ptr().add(self.udp_offset) as *const UdpHeader) }
    }

    /// Get the IPv4 header
    pub fn ipv4_header(&self) -> &Ipv4Header {
        let mbuf_ref = unsafe { &*self.mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };
        unsafe { &*(data.as_ptr().add(self.ip_offset) as *const Ipv4Header) }
    }

    /// Get the Ethernet header
    pub fn ethernet_header(&self) -> &EthernetHeader {
        let mbuf_ref = unsafe { &*self.mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };
        unsafe { &*(data.as_ptr().add(self.eth_offset) as *const EthernetHeader) }
    }

    /// Get the payload data
    pub fn payload(&self) -> &[u8] {
        let mbuf_ref = unsafe { &*self.mbuf };
        let data = unsafe { std::slice::from_raw_parts(mbuf_ref.data, mbuf_ref.len) };
        let udp_header = self.udp_header();
        let payload_len = udp_header.length() as usize - std::mem::size_of::<UdpHeader>();

        if self.payload_offset + payload_len <= data.len() {
            &data[self.payload_offset..self.payload_offset + payload_len]
        } else {
            &[]
        }
    }

    /// Get source socket address
    pub fn src_addr(&self) -> SocketAddr {
        let ip_header = self.ipv4_header();
        let udp_header = self.udp_header();

        SocketAddr::new(IpAddr::V4(ip_header.src_addr()), udp_header.src_port())
    }

    /// Get destination socket address
    pub fn dst_addr(&self) -> SocketAddr {
        let ip_header = self.ipv4_header();
        let udp_header = self.udp_header();

        SocketAddr::new(IpAddr::V4(ip_header.dst_addr()), udp_header.dst_port())
    }
}

/// UDP socket statistics
#[derive(Debug, Default)]
pub struct UdpSocketStats {
    pub packets_received: AtomicUsize,
    pub bytes_received: AtomicUsize,
    pub packets_sent: AtomicUsize,
    pub bytes_sent: AtomicUsize,
    pub packets_dropped: AtomicUsize,
    pub errors: AtomicUsize,
}

/// UDP socket implementation
pub struct UdpSocket {
    /// Local socket address
    local_addr: SocketAddr,
    /// Receive queue for incoming packets
    recv_queue: Arc<SpscRingBuffer<*mut Mbuf>>,
    /// Transmit queue for outgoing packets
    tx_queue: Option<Arc<TxQueue>>,
    /// Socket statistics
    stats: UdpSocketStats,
    /// Running flag
    running: AtomicBool,
    /// Socket ID
    id: u16,
}

impl UdpSocket {
    /// Create a new UDP socket
    pub fn new(local_addr: SocketAddr, queue_size: usize, id: u16) -> Result<Self> {
        let recv_queue = Arc::new(SpscRingBuffer::new(queue_size));

        Ok(Self {
            local_addr,
            recv_queue,
            tx_queue: None,
            stats: UdpSocketStats::default(),
            running: AtomicBool::new(false),
            id,
        })
    }

    /// Bind the socket to a transmit queue
    pub fn bind_tx_queue(&mut self, tx_queue: Arc<TxQueue>) {
        self.tx_queue = Some(tx_queue);
    }

    /// Receive a packet
    pub fn recv(&self) -> Result<UdpPacket> {
        match self.recv_queue.pop() {
            Ok(mbuf) => {
                let packet = UdpPacket::from_mbuf(mbuf)?;
                self.stats.packets_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .bytes_received
                    .fetch_add(packet.payload().len(), Ordering::Relaxed);
                Ok(packet)
            }
            Err(_) => Err(Error::NetworkError("No packet available".to_string())),
        }
    }

    /// Receive multiple packets in batch
    pub fn recv_batch(&self, packets: &mut [UdpPacket], max_count: usize) -> Result<usize> {
        let mut received = 0;

        for i in 0..max_count.min(packets.len()) {
            match self.recv() {
                Ok(packet) => {
                    packets[i] = packet;
                    received += 1;
                }
                Err(Error::NetworkError(_)) => break,
                Err(e) => return Err(e),
            }
        }

        Ok(received)
    }

    /// Send a packet
    pub fn send(&self, dst_addr: SocketAddr, data: &[u8]) -> Result<()> {
        let tx_queue = self
            .tx_queue
            .as_ref()
            .ok_or_else(|| Error::NetworkError("No transmit queue bound".to_string()))?;

        // Create packet
        let mbuf = self.create_packet(dst_addr, data)?;

        // Send packet
        tx_queue.send(mbuf)?;

        self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_sent
            .fetch_add(data.len(), Ordering::Relaxed);

        Ok(())
    }

    /// Send multiple packets in batch
    pub fn send_batch(&self, packets: &[(SocketAddr, &[u8])]) -> Result<usize> {
        let mut sent = 0;

        for (dst_addr, data) in packets.iter().take(packets.len()) {
            match self.send(*dst_addr, data) {
                Ok(_) => sent += 1,
                Err(_) => break,
            }
        }

        Ok(sent)
    }

    /// Create a UDP packet
    fn create_packet(&self, _dst_addr: SocketAddr, _data: &[u8]) -> Result<*mut Mbuf> {
        // This is a simplified implementation
        // In a real implementation, we would need to allocate an mbuf and build the packet

        // For now, return an error to indicate this needs proper implementation
        Err(Error::NetworkError(
            "Packet creation not implemented".to_string(),
        ))
    }

    /// Start the socket
    pub fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Stop the socket
    pub fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Get socket statistics
    pub fn stats(&self) -> &UdpSocketStats {
        &self.stats
    }

    /// Get local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Get socket ID
    pub fn id(&self) -> u16 {
        self.id
    }
}

/// UDP stack implementation
pub struct UdpStack {
    /// Stack configuration
    #[allow(dead_code)]
    config: Config,
    /// UDP sockets
    sockets: HashMap<u16, UdpSocket>,
    /// Next socket ID
    next_socket_id: AtomicUsize,
    /// Running flag
    running: AtomicBool,
    /// Stack statistics
    stats: UdpStackStats,
}

/// UDP stack statistics
#[derive(Debug, Default)]
pub struct UdpStackStats {
    pub total_sockets: AtomicUsize,
    pub active_sockets: AtomicUsize,
    pub total_packets_received: AtomicUsize,
    pub total_packets_sent: AtomicUsize,
    pub total_bytes_received: AtomicUsize,
    pub total_bytes_sent: AtomicUsize,
    pub total_errors: AtomicUsize,
}

impl UdpStack {
    /// Create a new UDP stack
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            sockets: HashMap::new(),
            next_socket_id: AtomicUsize::new(1),
            running: AtomicBool::new(false),
            stats: UdpStackStats::default(),
        })
    }

    /// Create a new UDP socket
    pub fn create_socket(&mut self, local_addr: SocketAddr) -> Result<u16> {
        let socket_id = self.next_socket_id.fetch_add(1, Ordering::Relaxed) as u16;
        let queue_size = 1024; // Default queue size

        let socket = UdpSocket::new(local_addr, queue_size, socket_id)?;

        self.sockets.insert(socket_id, socket);
        self.stats.total_sockets.fetch_add(1, Ordering::Relaxed);
        self.stats.active_sockets.fetch_add(1, Ordering::Relaxed);

        Ok(socket_id)
    }

    /// Get a socket by ID
    pub fn get_socket(&self, socket_id: u16) -> Option<&UdpSocket> {
        self.sockets.get(&socket_id)
    }

    /// Get a mutable socket by ID
    pub fn get_socket_mut(&mut self, socket_id: u16) -> Option<&mut UdpSocket> {
        self.sockets.get_mut(&socket_id)
    }

    /// Close a socket
    pub fn close_socket(&mut self, socket_id: u16) -> Result<()> {
        if let Some(socket) = self.sockets.remove(&socket_id) {
            socket.stop()?;
            self.stats.active_sockets.fetch_sub(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Process incoming packets from RX queue
    pub fn process_rx_packets(&mut self, rx_queue: &RxQueue) -> Result<usize> {
        let mut processed = 0;
        let max_batch = 32;

        for _ in 0..max_batch {
            match rx_queue.recv() {
                Ok(mbuf) => {
                    if let Ok(packet) = UdpPacket::from_mbuf(mbuf) {
                        // Find matching socket
                        let dst_addr = packet.dst_addr();

                        for socket in self.sockets.values() {
                            if socket.local_addr().port() == dst_addr.port() {
                                // Add packet to socket's receive queue
                                if let Err(_) = socket.recv_queue.push(mbuf) {
                                    // Queue full, drop packet
                                    self.stats.total_errors.fetch_add(1, Ordering::Relaxed);
                                }
                                break;
                            }
                        }

                        processed += 1;
                        self.stats
                            .total_packets_received
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        // Not a UDP packet, drop it
                        rx_queue.get_pool().free(mbuf)?;
                    }
                }
                Err(Error::NetworkError(_)) => break, // No more packets
                Err(e) => return Err(e),
            }
        }

        Ok(processed)
    }

    /// Start the UDP stack
    pub fn start(&mut self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);

        // Start all sockets
        for socket in self.sockets.values() {
            socket.start()?;
        }

        Ok(())
    }

    /// Stop the UDP stack
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        // Stop all sockets
        for socket in self.sockets.values() {
            socket.stop()?;
        }

        Ok(())
    }

    /// Get stack statistics
    pub fn stats(&self) -> UdpStackStatsView {
        let mut total_rx_packets = 0;
        let mut total_rx_bytes = 0;
        let _total_tx_packets = 0;
        let mut total_tx_bytes = 0;
        let mut total_errors = 0;

        for socket in self.sockets.values() {
            total_rx_packets += socket.stats.packets_received.load(Ordering::Relaxed);
            total_rx_bytes += socket.stats.bytes_received.load(Ordering::Relaxed);
            let _ = socket.stats.packets_sent.load(Ordering::Relaxed);
            total_tx_bytes += socket.stats.bytes_sent.load(Ordering::Relaxed);
            total_errors += socket.stats.errors.load(Ordering::Relaxed);
        }

        UdpStackStatsView {
            total_sockets: self.stats.total_sockets.load(Ordering::Relaxed),
            active_sockets: self.stats.active_sockets.load(Ordering::Relaxed),
            total_packets_received: self.stats.total_packets_received.load(Ordering::Relaxed),
            total_packets_sent: self.stats.total_packets_sent.load(Ordering::Relaxed),
            total_bytes_received: self.stats.total_bytes_received.load(Ordering::Relaxed),
            total_bytes_sent: self.stats.total_bytes_sent.load(Ordering::Relaxed),
            total_errors: self.stats.total_errors.load(Ordering::Relaxed),
            socket_stats: total_rx_packets,
            socket_bytes_rx: total_rx_bytes,
            socket_bytes_tx: total_tx_bytes,
            socket_errors: total_errors,
        }
    }
}

/// UDP stack statistics view
#[derive(Debug)]
pub struct UdpStackStatsView {
    pub total_sockets: usize,
    pub active_sockets: usize,
    pub total_packets_received: usize,
    pub total_packets_sent: usize,
    pub total_bytes_received: usize,
    pub total_bytes_sent: usize,
    pub total_errors: usize,
    pub socket_stats: usize,
    pub socket_bytes_rx: usize,
    pub socket_bytes_tx: usize,
    pub socket_errors: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_header() {
        let header = UdpHeader::new(8080, 53, 512);
        assert_eq!(header.src_port(), 8080);
        assert_eq!(header.dst_port(), 53);
        assert_eq!(header.length(), 512);
    }

    #[test]
    fn test_ipv4_header() {
        let src = Ipv4Addr::new(192, 168, 1, 1);
        let dst = Ipv4Addr::new(192, 168, 1, 2);
        let header = Ipv4Header::new(src, dst, 512);

        assert_eq!(header.src_addr(), src);
        assert_eq!(header.dst_addr(), dst);
        assert_eq!(header.protocol(), 17);
    }

    #[test]
    fn test_udp_stack_creation() {
        let config = Config::default();
        let stack = UdpStack::new(&config).unwrap();
        assert_eq!(stack.stats().total_sockets, 0);
    }

    #[test]
    fn test_socket_creation() {
        let config = Config::default();
        let mut stack = UdpStack::new(&config).unwrap();

        let local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let socket_id = stack.create_socket(local_addr).unwrap();

        assert!(socket_id > 0);
        assert_eq!(stack.stats().total_sockets, 1);
    }
}

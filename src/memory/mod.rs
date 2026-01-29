//! Memory management module with huge pages support and cache-line optimization

use crate::{Config, Error, Result};
use libc::{c_void, MAP_ANONYMOUS, MAP_FAILED, MAP_HUGETLB, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use nix::unistd::sysconf;
use nix::unistd::SysconfVar;
use parking_lot::Mutex;
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Cache line size for optimization (typically 64 bytes)
pub const CACHE_LINE_SIZE: usize = 64;

/// Page size information
#[derive(Debug, Clone)]
pub struct PageInfo {
    /// Regular page size (usually 4KB)
    pub regular_size: usize,
    /// Huge page size (usually 2MB)
    pub huge_size: usize,
}

impl PageInfo {
    /// Get system page information
    pub fn new() -> Result<Self> {
        let page_size = sysconf(SysconfVar::PAGE_SIZE)
            .unwrap_or(Some(4096))
            .unwrap_or(0) as usize;
        let regular_size = page_size;
        let huge_size = page_size * 512; // Assume 2MB huge pages

        Ok(Self {
            regular_size,
            huge_size,
        })
    }
}

/// Huge page memory allocator
pub struct HugePageAllocator {
    page_size: usize,
    allocated_blocks: AtomicUsize,
    total_allocated: AtomicUsize,
}

impl HugePageAllocator {
    /// Create a new huge page allocator
    pub fn new() -> Result<Self> {
        let page_info = PageInfo::new()?;

        Ok(Self {
            page_size: page_info.huge_size,
            allocated_blocks: AtomicUsize::new(0),
            total_allocated: AtomicUsize::new(0),
        })
    }

    /// Allocate memory using huge pages
    pub fn allocate(&self, size: usize) -> Result<*mut c_void> {
        // Round up to page size
        let aligned_size = ((size + self.page_size - 1) / self.page_size) * self.page_size;

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                aligned_size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS | MAP_HUGETLB,
                -1,
                0,
            )
        };

        if ptr == MAP_FAILED {
            // Fallback to regular pages if huge pages fail
            let fallback_ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    aligned_size,
                    PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };

            if fallback_ptr == MAP_FAILED {
                return Err(Error::MemoryAllocation(
                    "Failed to allocate memory".to_string(),
                ));
            }

            self.allocated_blocks.fetch_add(1, Ordering::Relaxed);
            self.total_allocated
                .fetch_add(aligned_size, Ordering::Relaxed);
            Ok(fallback_ptr)
        } else {
            self.allocated_blocks.fetch_add(1, Ordering::Relaxed);
            self.total_allocated
                .fetch_add(aligned_size, Ordering::Relaxed);
            Ok(ptr)
        }
    }

    /// Deallocate memory
    pub fn deallocate(&self, ptr: *mut c_void, size: usize) -> Result<()> {
        let aligned_size = ((size + self.page_size - 1) / self.page_size) * self.page_size;

        unsafe {
            if libc::munmap(ptr, aligned_size) == -1 {
                return Err(Error::MemoryAllocation(
                    "Failed to deallocate memory".to_string(),
                ));
            }
        }

        self.allocated_blocks.fetch_sub(1, Ordering::Relaxed);
        self.total_allocated
            .fetch_sub(aligned_size, Ordering::Relaxed);
        Ok(())
    }

    /// Get allocation statistics
    pub fn stats(&self) -> AllocationStats {
        AllocationStats {
            allocated_blocks: self.allocated_blocks.load(Ordering::Relaxed),
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
            page_size: self.page_size,
        }
    }
}

/// Allocation statistics
#[derive(Debug)]
pub struct AllocationStats {
    pub allocated_blocks: usize,
    pub total_allocated: usize,
    pub page_size: usize,
}

/// Memory buffer (mbuf) structure
#[repr(C, align(64))] // Cache line alignment
pub struct Mbuf {
    /// Data pointer
    pub data: *mut u8,
    /// Data length
    pub len: usize,
    /// Total buffer size
    pub buf_len: usize,
    /// Packet type
    pub packet_type: PacketType,
    /// Offload flags
    pub offload_flags: OffloadFlags,
    /// Timestamp
    pub timestamp: u64,
    /// Queue ID
    pub queue_id: u16,
    /// Reserved for future use
    _padding: [u8; 64 - 56], // Pad to cache line size
}

impl Mbuf {
    /// Create a new mbuf
    pub fn new(data: *mut u8, buf_len: usize) -> Self {
        Self {
            data,
            len: 0,
            buf_len,
            packet_type: PacketType::Unknown,
            offload_flags: OffloadFlags::empty(),
            timestamp: 0,
            queue_id: 0,
            _padding: [0; 8],
        }
    }

    /// Get data as slice
    pub fn data(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }

    /// Get mutable data as slice
    pub fn data_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.data, self.len) }
    }

    /// Append data to mbuf
    pub fn append(&mut self, data: &[u8]) -> Result<()> {
        if self.len + data.len() > self.buf_len {
            return Err(Error::MemoryAllocation("Mbuf overflow".to_string()));
        }

        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.data.add(self.len), data.len());
        }
        self.len += data.len();
        Ok(())
    }

    /// Reset mbuf
    pub fn reset(&mut self) {
        self.len = 0;
        self.packet_type = PacketType::Unknown;
        self.offload_flags = OffloadFlags::empty();
        self.timestamp = 0;
        self.queue_id = 0;
    }
}

unsafe impl Send for Mbuf {}
unsafe impl Sync for Mbuf {}

/// Packet type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PacketType {
    Unknown,
    Ethernet,
    Ipv4,
    Ipv6,
    Udp,
    Tcp,
    Icmp,
}

impl Default for PacketType {
    fn default() -> Self {
        Self::Unknown
    }
}

// Offload flags
bitflags::bitflags! {
    pub struct OffloadFlags: u32 {
        const CHECKSUM_OFFLOAD = 0x01;
        const TCP_SEGMENTATION_OFFLOAD = 0x02;
        const UDP_SEGMENTATION_OFFLOAD = 0x04;
        const RSS_HASH = 0x08;
        const TIMESTAMP = 0x10;
    }
}

/// Memory pool for mbufs
pub struct MbufPool {
    /// Pool name
    name: String,
    /// Pool size
    size: usize,
    /// Buffer size
    buf_size: usize,
    /// Memory allocator
    #[allow(dead_code)]
    allocator: HugePageAllocator,
    /// Free list (using atomic stack for lock-free access)
    free_list: AtomicPtr<Mbuf>,
    /// Pool metadata
    metadata: UnsafeCell<PoolMetadata>,
    /// Mutex for thread-safe operations
    #[allow(dead_code)]
    mutex: Mutex<()>,
}

#[derive(Debug)]
struct PoolMetadata {
    /// Total allocated mbufs
    allocated: usize,
    /// Available mbufs
    available: usize,
    /// Peak usage
    peak_usage: usize,
}

impl MbufPool {
    /// Create a new mbuf pool
    pub fn new(name: String, size: usize, buf_size: usize) -> Result<Self> {
        let allocator = HugePageAllocator::new()?;
        let total_memory = size * (std::mem::size_of::<Mbuf>() + buf_size);

        // Allocate memory for mbufs and data buffers
        let memory_base = allocator.allocate(total_memory)? as *mut u8;

        // Initialize mbufs
        let mbufs_ptr = memory_base as *mut Mbuf;
        let data_ptr = unsafe { memory_base.add(size * std::mem::size_of::<Mbuf>()) };

        // Build free list
        let mut free_head: *mut Mbuf = ptr::null_mut();
        for i in 0..size {
            let mbuf_ptr = unsafe { mbufs_ptr.add(i) };
            let mbuf_data = unsafe { data_ptr.add(i * buf_size) };

            unsafe {
                ptr::write(mbuf_ptr, Mbuf::new(mbuf_data, buf_size));
            }

            // Add to free list (push to front)
            unsafe {
                (*mbuf_ptr).data = mbuf_data;
                let _next_ptr = (*mbuf_ptr).data as *mut Mbuf;
                ptr::write(mbuf_ptr as *mut *mut Mbuf, free_head);
                free_head = mbuf_ptr;
            }
        }

        Ok(Self {
            name,
            size,
            buf_size,
            allocator,
            free_list: AtomicPtr::new(free_head),
            metadata: UnsafeCell::new(PoolMetadata {
                allocated: size,
                available: size,
                peak_usage: 0,
            }),
            mutex: Mutex::new(()),
        })
    }

    /// Allocate an mbuf from the pool
    pub fn alloc(&self) -> Result<*mut Mbuf> {
        loop {
            let current_head = self.free_list.load(Ordering::Acquire);
            if current_head.is_null() {
                return Err(Error::MemoryAllocation("Pool exhausted".to_string()));
            }

            let next = unsafe { *(current_head as *const *mut Mbuf) };

            if self
                .free_list
                .compare_exchange_weak(current_head, next, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                let metadata = unsafe { &mut *self.metadata.get() };
                metadata.available = metadata.available.saturating_sub(1);
                metadata.peak_usage = metadata.peak_usage.max(self.size - metadata.available);
                return Ok(current_head);
            }
        }
    }

    /// Free an mbuf back to the pool
    pub fn free(&self, mbuf: *mut Mbuf) -> Result<()> {
        if mbuf.is_null() {
            return Ok(());
        }

        // Reset mbuf
        unsafe {
            (*mbuf).reset();
        }

        loop {
            let current_head = self.free_list.load(Ordering::Acquire);

            unsafe {
                *(mbuf as *mut *mut Mbuf) = current_head;
            }

            if self
                .free_list
                .compare_exchange_weak(current_head, mbuf, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                let metadata = unsafe { &mut *self.metadata.get() };
                metadata.available = metadata.available.saturating_add(1);
                return Ok(());
            }
        }
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let metadata = unsafe { &*self.metadata.get() };
        PoolStats {
            name: self.name.clone(),
            size: self.size,
            buf_size: self.buf_size,
            allocated: metadata.allocated,
            available: metadata.available,
            in_use: metadata.allocated - metadata.available,
            peak_usage: metadata.peak_usage,
        }
    }
}

/// Pool statistics
#[derive(Debug)]
pub struct PoolStats {
    pub name: String,
    pub size: usize,
    pub buf_size: usize,
    pub allocated: usize,
    pub available: usize,
    pub in_use: usize,
    pub peak_usage: usize,
}

/// Memory manager for the entire system
pub struct MemoryManager {
    #[allow(dead_code)]
    config: Config,
    pools: Vec<MbufPool>,
    allocator: HugePageAllocator,
}

impl MemoryManager {
    /// Create a new memory manager
    pub fn new(config: &Config) -> Result<Self> {
        let allocator = HugePageAllocator::new()?;
        let mut pools = Vec::with_capacity(config.pool_count);

        for i in 0..config.pool_count {
            let pool = MbufPool::new(
                format!("pool_{}", i),
                config.pool_size,
                2048, // Default buffer size: 2KB
            )?;
            pools.push(pool);
        }

        Ok(Self {
            config: config.clone(),
            pools,
            allocator,
        })
    }

    /// Get a memory pool by index
    pub fn get_pool(&self, index: usize) -> Option<&MbufPool> {
        self.pools.get(index)
    }

    /// Allocate an mbuf from the best available pool
    pub fn alloc_mbuf(&self) -> Result<*mut Mbuf> {
        for pool in &self.pools {
            match pool.alloc() {
                Ok(mbuf) => return Ok(mbuf),
                Err(_) => continue,
            }
        }
        Err(Error::MemoryAllocation(
            "No available mbufs in any pool".to_string(),
        ))
    }

    /// Free an mbuf back to its pool
    pub fn free_mbuf(&self, mbuf: *mut Mbuf) -> Result<()> {
        // In a real implementation, we would track which pool this mbuf came from
        // For now, use the first pool
        if let Some(pool) = self.pools.first() {
            pool.free(mbuf)
        } else {
            Err(Error::MemoryAllocation("No pools available".to_string()))
        }
    }

    /// Get memory statistics
    pub fn stats(&self) -> MemoryStats {
        let alloc_stats = self.allocator.stats();
        let mut pool_stats = Vec::new();

        for pool in &self.pools {
            pool_stats.push(pool.stats());
        }

        MemoryStats {
            allocation: alloc_stats,
            pools: pool_stats,
        }
    }
}

/// Memory statistics
#[derive(Debug)]
pub struct MemoryStats {
    pub allocation: AllocationStats,
    pub pools: Vec<PoolStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_info() {
        let info = PageInfo::new().unwrap();
        assert!(info.regular_size > 0);
        assert!(info.huge_size > info.regular_size);
    }

    #[test]
    fn test_huge_page_allocator() {
        let allocator = HugePageAllocator::new().unwrap();
        let ptr = allocator.allocate(1024).unwrap();
        assert!(!ptr.is_null());
        allocator.deallocate(ptr, 1024).unwrap();
    }

    #[test]
    fn test_mbuf_operations() {
        let data = vec![0u8; 2048];
        let mut mbuf = Mbuf::new(data.as_ptr() as *mut u8, 2048);

        let test_data = b"Hello, World!";
        mbuf.append(test_data).unwrap();
        assert_eq!(mbuf.data(), test_data);

        mbuf.reset();
        assert_eq!(mbuf.len, 0);
    }

    #[test]
    fn test_mbuf_pool() {
        let pool = MbufPool::new("test".to_string(), 16, 1024).unwrap();
        let mbuf = pool.alloc().unwrap();
        assert!(!mbuf.is_null());
        pool.free(mbuf).unwrap();

        let stats = pool.stats();
        assert_eq!(stats.size, 16);
        assert_eq!(stats.available, 16);
    }
}

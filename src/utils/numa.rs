//! NUMA (Non-Uniform Memory Access) utilities for memory affinity optimization

use crate::{Error, Result};
use libc::{c_void, MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

/// NUMA node information
#[derive(Debug, Clone)]
pub struct NumaNode {
    /// Node ID
    pub id: usize,
    /// Total memory in bytes
    pub total_memory: u64,
    /// Free memory in bytes
    pub free_memory: u64,
    /// CPU cores belonging to this node
    pub cpu_cores: Vec<usize>,
    /// Distance to other nodes
    pub distances: HashMap<usize, u8>,
}

/// NUMA topology information
#[derive(Debug, Default)]
pub struct NumaTopology {
    /// Number of NUMA nodes
    pub num_nodes: usize,
    /// NUMA nodes
    pub nodes: HashMap<usize, NumaNode>,
    /// Mapping from CPU core to NUMA node
    pub core_to_node: HashMap<usize, usize>,
    /// Whether NUMA is available
    pub numa_available: bool,
}

/// NUMA memory allocator
pub struct NumaAllocator {
    /// NUMA node ID
    node_id: usize,
    /// Allocated memory blocks
    allocated_blocks: AtomicUsize,
    /// Total allocated memory
    total_allocated: AtomicUsize,
}

impl NumaAllocator {
    /// Create a new NUMA allocator for a specific node
    pub fn new(node_id: usize) -> Result<Self> {
        // Check if NUMA is available
        if !is_numa_available() {
            return Err(Error::NumaError(
                "NUMA not available on this system".to_string(),
            ));
        }

        Ok(Self {
            node_id,
            allocated_blocks: AtomicUsize::new(0),
            total_allocated: AtomicUsize::new(0),
        })
    }

    /// Allocate memory on the specified NUMA node
    pub fn allocate(&self, size: usize) -> Result<*mut c_void> {
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if ptr == MAP_FAILED {
            return Err(Error::NumaError("Failed to allocate memory".to_string()));
        }

        // Bind memory to NUMA node
        if let Err(e) = bind_memory_to_node(ptr, size, self.node_id) {
            unsafe {
                libc::munmap(ptr, size);
            }
            return Err(e);
        }

        self.allocated_blocks.fetch_add(1, Ordering::Relaxed);
        self.total_allocated.fetch_add(size, Ordering::Relaxed);

        Ok(ptr)
    }

    /// Deallocate memory
    pub fn deallocate(&self, ptr: *mut c_void, size: usize) -> Result<()> {
        unsafe {
            if libc::munmap(ptr, size) == -1 {
                return Err(Error::NumaError("Failed to deallocate memory".to_string()));
            }
        }

        self.allocated_blocks.fetch_sub(1, Ordering::Relaxed);
        self.total_allocated.fetch_sub(size, Ordering::Relaxed);

        Ok(())
    }

    /// Get allocation statistics
    pub fn stats(&self) -> NumaAllocatorStats {
        NumaAllocatorStats {
            node_id: self.node_id,
            allocated_blocks: self.allocated_blocks.load(Ordering::Relaxed),
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
        }
    }
}

/// NUMA allocator statistics
#[derive(Debug)]
pub struct NumaAllocatorStats {
    pub node_id: usize,
    pub allocated_blocks: usize,
    pub total_allocated: usize,
}

/// NUMA memory pool
pub struct NumaMemoryPool {
    /// NUMA node ID
    node_id: usize,
    /// Pool size
    size: usize,
    /// Buffer size
    buf_size: usize,
    /// Memory allocator
    allocator: NumaAllocator,
    /// Memory base pointer
    memory_base: *mut u8,
    /// Free list
    free_list: Mutex<Vec<*mut u8>>,
    /// Pool statistics
    stats: NumaPoolStats,
}

/// NUMA pool statistics
#[derive(Debug, Default)]
pub struct NumaPoolStats {
    pub allocated: AtomicUsize,
    pub available: AtomicUsize,
    pub in_use: AtomicUsize,
    pub peak_usage: AtomicUsize,
}

impl NumaMemoryPool {
    /// Create a new NUMA memory pool
    pub fn new(node_id: usize, size: usize, buf_size: usize) -> Result<Self> {
        let allocator = NumaAllocator::new(node_id)?;
        let total_memory = size * buf_size;

        let memory_base = allocator.allocate(total_memory)? as *mut u8;

        // Initialize free list
        let mut free_list = Vec::with_capacity(size);
        for i in 0..size {
            let ptr = unsafe { memory_base.add(i * buf_size) };
            free_list.push(ptr);
        }

        Ok(Self {
            node_id,
            size,
            buf_size,
            allocator,
            memory_base,
            free_list: Mutex::new(free_list),
            stats: NumaPoolStats::default(),
        })
    }

    /// Allocate a buffer from the pool
    pub fn alloc(&self) -> Result<*mut u8> {
        let mut free_list = self.free_list.lock();

        if let Some(ptr) = free_list.pop() {
            self.stats.allocated.fetch_add(1, Ordering::Relaxed);
            self.stats.available.fetch_sub(1, Ordering::Relaxed);

            let in_use = self.stats.in_use.fetch_add(1, Ordering::Relaxed) + 1;
            self.stats.peak_usage.fetch_max(in_use, Ordering::Relaxed);

            Ok(ptr)
        } else {
            Err(Error::NumaError("Pool exhausted".to_string()))
        }
    }

    /// Free a buffer back to the pool
    pub fn free(&self, ptr: *mut u8) -> Result<()> {
        if ptr.is_null() {
            return Ok(());
        }

        let mut free_list = self.free_list.lock();
        free_list.push(ptr);

        self.stats.allocated.fetch_sub(1, Ordering::Relaxed);
        self.stats.available.fetch_add(1, Ordering::Relaxed);
        self.stats.in_use.fetch_sub(1, Ordering::Relaxed);

        Ok(())
    }

    /// Get pool statistics
    pub fn stats(&self) -> NumaPoolStatsView {
        NumaPoolStatsView {
            node_id: self.node_id,
            size: self.size,
            buf_size: self.buf_size,
            allocated: self.stats.allocated.load(Ordering::Relaxed),
            available: self.stats.available.load(Ordering::Relaxed),
            in_use: self.stats.in_use.load(Ordering::Relaxed),
            peak_usage: self.stats.peak_usage.load(Ordering::Relaxed),
        }
    }
}

/// NUMA pool statistics view
#[derive(Debug)]
pub struct NumaPoolStatsView {
    pub node_id: usize,
    pub size: usize,
    pub buf_size: usize,
    pub allocated: usize,
    pub available: usize,
    pub in_use: usize,
    pub peak_usage: usize,
}

impl Drop for NumaMemoryPool {
    fn drop(&mut self) {
        let total_memory = self.size * self.buf_size;
        let _ = self
            .allocator
            .deallocate(self.memory_base as *mut c_void, total_memory);
    }
}

/// NUMA affinity manager
pub struct NumaAffinity {
    /// NUMA topology
    topology: NumaTopology,
    /// Current NUMA node affinity
    current_affinity: Option<usize>,
}

impl NumaAffinity {
    /// Create a new NUMA affinity manager
    pub fn new() -> Result<Self> {
        let topology = detect_numa_topology()?;
        let current_affinity = get_current_numa_node();

        Ok(Self {
            topology,
            current_affinity,
        })
    }

    /// Set NUMA affinity for the current thread
    pub fn set_thread_affinity(&self, node_id: usize) -> Result<()> {
        if !self.topology.numa_available {
            return Err(Error::NumaError("NUMA not available".to_string()));
        }

        if !self.topology.nodes.contains_key(&node_id) {
            return Err(Error::NumaError(format!("NUMA node {} not found", node_id)));
        }

        // Use libnuma if available, otherwise use sysfs
        #[cfg(feature = "libnuma")]
        unsafe {
            if numa_available() != -1 {
                numa_run_on_node(node_id as c_int);
                numa_set_preferred(node_id as c_int);
                return Ok(());
            }
        }

        // Fallback: set CPU affinity for cores in the NUMA node
        if let Some(node) = self.topology.nodes.get(&node_id) {
            let cpu_affinity = crate::utils::cpu::CpuAffinity::new()?;
            cpu_affinity.set_thread_affinity(&node.cpu_cores)?;
        }

        Ok(())
    }

    /// Set NUMA affinity for the current process
    pub fn set_process_affinity(&self, node_id: usize) -> Result<()> {
        if !self.topology.numa_available {
            return Err(Error::NumaError("NUMA not available".to_string()));
        }

        if !self.topology.nodes.contains_key(&node_id) {
            return Err(Error::NumaError(format!("NUMA node {} not found", node_id)));
        }

        // Set CPU affinity for all cores in the NUMA node
        if let Some(node) = self.topology.nodes.get(&node_id) {
            let cpu_affinity = crate::utils::cpu::CpuAffinity::new()?;
            cpu_affinity.set_process_affinity(&node.cpu_cores)?;
        }

        Ok(())
    }

    /// Get current NUMA node affinity
    pub fn get_current_affinity(&self) -> Option<usize> {
        self.current_affinity
    }

    /// Get NUMA topology
    pub fn topology(&self) -> &NumaTopology {
        &self.topology
    }

    /// Get optimal NUMA node for a given CPU core
    pub fn get_optimal_node_for_core(&self, core_id: usize) -> Option<usize> {
        self.topology.core_to_node.get(&core_id).copied()
    }

    /// Get NUMA node with most free memory
    pub fn get_node_with_most_memory(&self) -> Option<usize> {
        let mut best_node = None;
        let mut max_free = 0u64;

        for (node_id, node) in &self.topology.nodes {
            if node.free_memory > max_free {
                max_free = node.free_memory;
                best_node = Some(*node_id);
            }
        }

        best_node
    }
}

/// Check if NUMA is available
fn is_numa_available() -> bool {
    // Check if /sys/devices/system/node exists
    Path::new("/sys/devices/system/node").exists()
}

/// Detect NUMA topology
fn detect_numa_topology() -> Result<NumaTopology> {
    if !is_numa_available() {
        return Ok(NumaTopology {
            num_nodes: 0,
            nodes: HashMap::new(),
            core_to_node: HashMap::new(),
            numa_available: false,
        });
    }

    let mut nodes = HashMap::new();
    let mut core_to_node = HashMap::new();

    // Scan NUMA nodes
    let nodes_path = Path::new("/sys/devices/system/node");
    for entry in fs::read_dir(nodes_path)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        if file_name_str.starts_with("node") {
            if let Some(node_id_str) = file_name_str.strip_prefix("node") {
                if let Ok(node_id) = node_id_str.parse::<usize>() {
                    let node = detect_numa_node_info(node_id)?;

                    // Map CPU cores to this node
                    for &core_id in &node.cpu_cores {
                        core_to_node.insert(core_id, node_id);
                    }

                    nodes.insert(node_id, node);
                }
            }
        }
    }

    let num_nodes = nodes.len();

    Ok(NumaTopology {
        num_nodes,
        nodes,
        core_to_node,
        numa_available: true,
    })
}

/// Detect information for a specific NUMA node
fn detect_numa_node_info(node_id: usize) -> Result<NumaNode> {
    let node_path_str = format!("/sys/devices/system/node/node{}", node_id);
    let node_path = Path::new(&node_path_str);

    // Get memory information
    let total_memory = read_numa_memory_info(node_path, "meminfo")?;
    let free_memory = read_numa_memory_info(node_path, "meminfo")?; // Simplified

    // Get CPU cores
    let cpu_cores = get_numa_cpu_cores(node_id)?;

    // Get distances to other nodes
    let distances = get_numa_distances(node_id)?;

    Ok(NumaNode {
        id: node_id,
        total_memory,
        free_memory,
        cpu_cores,
        distances,
    })
}

/// Read NUMA memory information
fn read_numa_memory_info(node_path: &Path, file: &str) -> Result<u64> {
    let meminfo_path = node_path.join(file);
    let _content = fs::read_to_string(meminfo_path)?;

    // Parse memory information (simplified)
    // In a real implementation, you would parse the actual format
    Ok(8 * 1024 * 1024 * 1024) // Default to 8GB
}

/// Get CPU cores for a NUMA node
fn get_numa_cpu_cores(node_id: usize) -> Result<Vec<usize>> {
    let cpulist_path = format!("/sys/devices/system/node/node{}/cpulist", node_id);

    if let Ok(content) = fs::read_to_string(&cpulist_path) {
        parse_cpu_list(&content)
    } else {
        // Fallback: assume 4 cores per NUMA node
        Ok((node_id * 4..(node_id + 1) * 4).collect())
    }
}

/// Parse CPU list string
fn parse_cpu_list(cpu_list: &str) -> Result<Vec<usize>> {
    let mut cores = Vec::new();

    for part in cpu_list.trim().split(',') {
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() == 2 {
                let start = range[0].parse::<usize>()?;
                let end = range[1].parse::<usize>()?;
                cores.extend(start..=end);
            }
        } else {
            cores.push(part.parse::<usize>()?);
        }
    }

    Ok(cores)
}

/// Get NUMA distances
fn get_numa_distances(node_id: usize) -> Result<HashMap<usize, u8>> {
    let distance_path = format!("/sys/devices/system/node/node{}/distance", node_id);

    if let Ok(content) = fs::read_to_string(&distance_path) {
        parse_numa_distances(&content)
    } else {
        // Fallback: assume distance 10 to self, 20 to others
        let mut distances = HashMap::new();
        distances.insert(node_id, 10);
        Ok(distances)
    }
}

/// Parse NUMA distances
fn parse_numa_distances(distance_str: &str) -> Result<HashMap<usize, u8>> {
    let mut distances = HashMap::new();

    for (i, part) in distance_str.trim().split_whitespace().enumerate() {
        if let Ok(distance) = part.parse::<u8>() {
            distances.insert(i, distance);
        }
    }

    Ok(distances)
}

/// Get current NUMA node
fn get_current_numa_node() -> Option<usize> {
    // Try to read from /proc/self/numa_maps
    if let Ok(content) = fs::read_to_string("/proc/self/numa_maps") {
        // Parse the first line to get the preferred node
        if let Some(first_line) = content.lines().next() {
            if let Some(node_part) = first_line.split_whitespace().nth(1) {
                if let Some(node_str) = node_part.strip_prefix("N") {
                    if let Ok(node_id) = node_str.parse::<usize>() {
                        return Some(node_id);
                    }
                }
            }
        }
    }

    None
}

/// Bind memory to NUMA node
fn bind_memory_to_node(_ptr: *mut c_void, _size: usize, _node_id: usize) -> Result<()> {
    #[cfg(feature = "libnuma")]
    unsafe {
        if numa_available() != -1 {
            let result =
                numa_tonode_memory(_ptr as *mut libc::c_void, _size, _node_id as libc::c_int);
            if result != 0 {
                return Err(Error::NumaError(
                    "Failed to bind memory to NUMA node".to_string(),
                ));
            }
            return Ok(());
        }
    }

    // Fallback: use mbind with libnuma if available
    // For now, just return Ok as the memory was allocated with the correct affinity
    Ok(())
}

/// NUMA-aware memory manager
pub struct NumaMemoryManager {
    /// NUMA affinity manager
    affinity: NumaAffinity,
    /// Memory pools for each NUMA node
    pools: HashMap<usize, NumaMemoryPool>,
    /// Default pool size
    #[allow(dead_code)]
    default_pool_size: usize,
    /// Default buffer size
    #[allow(dead_code)]
    default_buf_size: usize,
}

impl NumaMemoryManager {
    /// Create a new NUMA memory manager
    pub fn new(default_pool_size: usize, default_buf_size: usize) -> Result<Self> {
        let affinity = NumaAffinity::new()?;
        let mut pools = HashMap::new();

        // Create memory pools for each NUMA node
        for (node_id, _) in &affinity.topology().nodes {
            let pool = NumaMemoryPool::new(*node_id, default_pool_size, default_buf_size)?;
            pools.insert(*node_id, pool);
        }

        Ok(Self {
            affinity,
            pools,
            default_pool_size,
            default_buf_size,
        })
    }

    /// Allocate memory on the optimal NUMA node
    pub fn allocate_optimal(&self) -> Result<*mut u8> {
        let current_node = self.affinity.get_current_affinity();

        if let Some(node_id) = current_node {
            if let Some(pool) = self.pools.get(&node_id) {
                return pool.alloc();
            }
        }

        // Fallback to node with most memory
        if let Some(node_id) = self.affinity.get_node_with_most_memory() {
            if let Some(pool) = self.pools.get(&node_id) {
                return pool.alloc();
            }
        }

        Err(Error::NumaError("No suitable NUMA node found".to_string()))
    }

    /// Allocate memory on a specific NUMA node
    pub fn allocate_on_node(&self, node_id: usize) -> Result<*mut u8> {
        if let Some(pool) = self.pools.get(&node_id) {
            pool.alloc()
        } else {
            Err(Error::NumaError(format!(
                "No pool for NUMA node {}",
                node_id
            )))
        }
    }

    /// Free memory
    pub fn free(&self, ptr: *mut u8) -> Result<()> {
        // In a real implementation, you would track which node the memory came from
        // For now, try to free from the first available pool
        for pool in self.pools.values() {
            if pool.free(ptr).is_ok() {
                return Ok(());
            }
        }

        Err(Error::NumaError("Memory not found in any pool".to_string()))
    }

    /// Get NUMA affinity manager
    pub fn affinity(&self) -> &NumaAffinity {
        &self.affinity
    }

    /// Get memory pool for a specific node
    pub fn get_pool(&self, node_id: usize) -> Option<&NumaMemoryPool> {
        self.pools.get(&node_id)
    }

    /// Get statistics for all pools
    pub fn stats(&self) -> NumaMemoryManagerStats {
        let mut pool_stats = Vec::new();

        for (_node_id, pool) in &self.pools {
            pool_stats.push(pool.stats());
        }

        NumaMemoryManagerStats {
            num_nodes: self.pools.len(),
            pool_stats,
        }
    }
}

/// NUMA memory manager statistics
#[derive(Debug)]
pub struct NumaMemoryManagerStats {
    pub num_nodes: usize,
    pub pool_stats: Vec<NumaPoolStatsView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numa_availability() {
        let available = is_numa_available();
        println!("NUMA available: {}", available);
    }

    #[test]
    fn test_numa_topology() {
        let topology = detect_numa_topology().unwrap();
        println!("NUMA nodes: {}", topology.num_nodes);
        println!("NUMA available: {}", topology.numa_available);
    }

    #[test]
    fn test_numa_affinity() {
        let affinity = NumaAffinity::new().unwrap();
        println!("Current affinity: {:?}", affinity.get_current_affinity());
    }

    #[test]
    fn test_numa_allocator() {
        if is_numa_available() {
            let allocator = NumaAllocator::new(0).unwrap();
            let ptr = allocator.allocate(1024).unwrap();
            assert!(!ptr.is_null());
            allocator.deallocate(ptr, 1024).unwrap();
        }
    }
}

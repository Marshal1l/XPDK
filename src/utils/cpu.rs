//! CPU affinity and core binding utilities

use crate::{Error, Result};
use libc::{cpu_set_t, sched_getaffinity, sched_setaffinity};
use nix::unistd::getpid;
use std::collections::HashMap;

/// CPU information
#[derive(Debug, Clone)]
pub struct CpuInfo {
    /// CPU core ID
    pub core_id: usize,
    /// Physical package ID
    pub package_id: usize,
    /// NUMA node ID
    pub numa_node: Option<usize>,
    /// CPU frequency in MHz
    pub frequency: Option<u64>,
    /// Cache line size
    pub cache_line_size: usize,
    /// L1 cache size in bytes
    pub l1_cache_size: usize,
    /// L2 cache size in bytes
    pub l2_cache_size: usize,
    /// L3 cache size in bytes
    pub l3_cache_size: usize,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self {
            core_id: 0,
            package_id: 0,
            numa_node: None,
            frequency: None,
            cache_line_size: 64,
            l1_cache_size: 32 * 1024,
            l2_cache_size: 256 * 1024,
            l3_cache_size: 8 * 1024 * 1024,
        }
    }
}

/// CPU topology information
#[derive(Debug, Default)]
pub struct CpuTopology {
    /// Number of CPU cores
    pub num_cores: usize,
    /// Number of physical packages
    pub num_packages: usize,
    /// Number of NUMA nodes
    pub num_numa_nodes: usize,
    /// CPU information for each core
    pub cpu_info: Vec<CpuInfo>,
    /// Mapping from core ID to NUMA node
    pub core_to_numa: HashMap<usize, usize>,
    /// Mapping from NUMA node to cores
    pub numa_to_cores: HashMap<usize, Vec<usize>>,
}

impl CpuTopology {
    /// Create a new CPU topology
    pub fn new() -> Result<Self> {
        let num_cores = num_cpus::get();
        let mut cpu_info = Vec::with_capacity(num_cores);
        let mut core_to_numa = HashMap::new();
        let mut numa_to_cores = HashMap::new();

        // Basic CPU information (simplified)
        for i in 0..num_cores {
            let numa_node = detect_numa_node(i);

            if let Some(node) = numa_node {
                core_to_numa.insert(i, node);
                numa_to_cores.entry(node).or_insert_with(Vec::new).push(i);
            }

            cpu_info.push(CpuInfo {
                core_id: i,
                package_id: i / 4, // Simplified: assume 4 cores per package
                numa_node,
                frequency: get_cpu_frequency(i),
                cache_line_size: 64,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
            });
        }

        let num_packages = (num_cores + 3) / 4; // Simplified
        let num_numa_nodes = numa_to_cores.len();

        Ok(Self {
            num_cores,
            num_packages,
            num_numa_nodes,
            cpu_info,
            core_to_numa,
            numa_to_cores,
        })
    }

    /// Get CPU information for a specific core
    pub fn get_cpu_info(&self, core_id: usize) -> Option<&CpuInfo> {
        self.cpu_info.get(core_id)
    }

    /// Get cores for a specific NUMA node
    pub fn get_numa_cores(&self, numa_node: usize) -> Option<&Vec<usize>> {
        self.numa_to_cores.get(&numa_node)
    }

    /// Get NUMA node for a specific core
    pub fn get_core_numa(&self, core_id: usize) -> Option<usize> {
        self.core_to_numa.get(&core_id).copied()
    }
}

/// CPU affinity manager
pub struct CpuAffinity {
    /// Current process affinity
    #[allow(dead_code)]
    current_affinity: Vec<usize>,
    /// CPU topology
    topology: CpuTopology,
}

impl CpuAffinity {
    /// Create a new CPU affinity manager
    pub fn new() -> Result<Self> {
        let topology = CpuTopology::new()?;
        let current_affinity = get_current_affinity()?;

        Ok(Self {
            current_affinity,
            topology,
        })
    }

    /// Set CPU affinity for the current thread
    pub fn set_thread_affinity(&self, core_ids: &[usize]) -> Result<()> {
        if core_ids.is_empty() {
            return Err(Error::InvalidConfig("Core IDs cannot be empty".to_string()));
        }

        let mut cpu_set: cpu_set_t = unsafe { std::mem::zeroed() };

        for &core_id in core_ids {
            if core_id >= self.topology.num_cores {
                return Err(Error::InvalidConfig(format!(
                    "Core ID {} out of range",
                    core_id
                )));
            }

            unsafe {
                libc::CPU_SET(core_id, &mut cpu_set);
            }
        }

        let result = unsafe {
            libc::pthread_setaffinity_np(
                libc::pthread_self(),
                std::mem::size_of::<cpu_set_t>(),
                &cpu_set as *const cpu_set_t,
            )
        };

        if result != 0 {
            return Err(Error::IoError(std::io::Error::last_os_error()));
        }

        Ok(())
    }

    /// Set CPU affinity for the current process
    pub fn set_process_affinity(&self, core_ids: &[usize]) -> Result<()> {
        if core_ids.is_empty() {
            return Err(Error::InvalidConfig("Core IDs cannot be empty".to_string()));
        }

        let mut cpu_set: cpu_set_t = unsafe { std::mem::zeroed() };

        for &core_id in core_ids {
            if core_id >= self.topology.num_cores {
                return Err(Error::InvalidConfig(format!(
                    "Core ID {} out of range",
                    core_id
                )));
            }

            unsafe {
                libc::CPU_SET(core_id, &mut cpu_set);
            }
        }

        let result = unsafe {
            sched_setaffinity(
                getpid().as_raw(),
                std::mem::size_of::<cpu_set_t>(),
                &cpu_set as *const cpu_set_t,
            )
        };

        if result != 0 {
            return Err(Error::IoError(std::io::Error::last_os_error()));
        }

        Ok(())
    }

    /// Get current thread affinity
    pub fn get_thread_affinity(&self) -> Result<Vec<usize>> {
        let mut cpu_set: cpu_set_t = unsafe { std::mem::zeroed() };

        let result = unsafe {
            libc::pthread_getaffinity_np(
                libc::pthread_self(),
                std::mem::size_of::<cpu_set_t>(),
                &mut cpu_set as *mut cpu_set_t,
            )
        };

        if result != 0 {
            return Err(Error::IoError(std::io::Error::last_os_error()));
        }

        let mut core_ids = Vec::new();
        for i in 0..self.topology.num_cores {
            if unsafe { libc::CPU_ISSET(i, &cpu_set) } {
                core_ids.push(i);
            }
        }

        Ok(core_ids)
    }

    /// Get CPU topology
    pub fn topology(&self) -> &CpuTopology {
        &self.topology
    }

    /// Get optimal core assignment for NUMA-aware processing
    pub fn get_numa_optimal_cores(&self, numa_node: usize, count: usize) -> Result<Vec<usize>> {
        if let Some(cores) = self.topology.get_numa_cores(numa_node) {
            if cores.len() >= count {
                Ok(cores[..count].to_vec())
            } else {
                Err(Error::InvalidConfig(format!(
                    "Not enough cores in NUMA node {}",
                    numa_node
                )))
            }
        } else {
            Err(Error::InvalidConfig(format!(
                "NUMA node {} not found",
                numa_node
            )))
        }
    }
}

/// Detect NUMA node for a CPU core
fn detect_numa_node(core_id: usize) -> Option<usize> {
    // This is a simplified implementation
    // In a real implementation, you would read from /sys/devices/system/node/

    #[cfg(feature = "numa")]
    {
        // Try to read NUMA node information
        let path = format!("/sys/devices/system/node/node{}/cpulist", core_id);
        if std::path::Path::new(&path).exists() {
            return Some(core_id / 8); // Simplified: assume 8 cores per NUMA node
        }
    }

    // Default to no NUMA node
    None
}

/// Get CPU frequency for a specific core
fn get_cpu_frequency(core_id: usize) -> Option<u64> {
    // Try to read from /sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq
    let path = format!(
        "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq",
        core_id
    );

    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(freq_khz) = content.trim().parse::<u64>() {
            return Some(freq_khz / 1000); // Convert to MHz
        }
    }

    // Fallback to default frequency
    None
}

/// Get current CPU affinity
fn get_current_affinity() -> Result<Vec<usize>> {
    let mut cpu_set: cpu_set_t = unsafe { std::mem::zeroed() };

    let result = unsafe {
        sched_getaffinity(
            getpid().as_raw(),
            std::mem::size_of::<cpu_set_t>(),
            &mut cpu_set as *mut cpu_set_t,
        )
    };

    if result != 0 {
        return Err(Error::IoError(std::io::Error::last_os_error()));
    }

    let num_cores = num_cpus::get();
    let mut core_ids = Vec::new();

    for i in 0..num_cores {
        if unsafe { libc::CPU_ISSET(i, &cpu_set) } {
            core_ids.push(i);
        }
    }

    Ok(core_ids)
}

/// CPU cache prefetch utilities
pub struct CpuPrefetch;

impl CpuPrefetch {
    /// Prefetch data to L1 cache
    #[inline]
    pub fn prefetch_l1<T>(ptr: *const T) {
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr as *const i8, core::arch::x86_64::_MM_HINT_T0);
        }
    }

    /// Prefetch data to L2 cache
    #[inline]
    pub fn prefetch_l2<T>(ptr: *const T) {
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr as *const i8, core::arch::x86_64::_MM_HINT_T1);
        }
    }

    /// Prefetch data to L3 cache
    #[inline]
    pub fn prefetch_l3<T>(ptr: *const T) {
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr as *const i8, core::arch::x86_64::_MM_HINT_T2);
        }
    }

    /// Prefetch data for non-temporal access
    #[inline]
    pub fn prefetch_non_temporal<T>(ptr: *const T) {
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr as *const i8, core::arch::x86_64::_MM_HINT_NTA);
        }
    }
}

/// CPU instruction set utilities
pub struct CpuInstructions;

impl CpuInstructions {
    /// Check if CPU supports AVX2
    pub fn has_avx2() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Check if CPU supports AVX512
    pub fn has_avx512() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("avx512f")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Check if CPU supports RDRAND
    pub fn has_rdrand() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("rdrand")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Check if CPU supports FMA
    pub fn has_fma() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("fma")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_topology() {
        let topology = CpuTopology::new().unwrap();
        assert!(topology.num_cores > 0);
        assert_eq!(topology.cpu_info.len(), topology.num_cores);
    }

    #[test]
    fn test_cpu_affinity() {
        let affinity = CpuAffinity::new().unwrap();
        let current = affinity.get_thread_affinity().unwrap();
        assert!(!current.is_empty());
    }

    #[test]
    fn test_cpu_instructions() {
        // These should not panic
        let _avx2 = CpuInstructions::has_avx2();
        let _avx512 = CpuInstructions::has_avx512();
        let _rdrand = CpuInstructions::has_rdrand();
        let _fma = CpuInstructions::has_fma();
    }
}

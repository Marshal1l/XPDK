//! Queue module providing lock-free ring buffers for high-performance concurrent operations
//!
//! This module wraps the existing lockfree-ringbuf crate and provides additional
//! queue implementations optimized for the XPDK use case.

use crate::{memory::Mbuf, Error, Result};
use lockfree_ringbuf::{BatchOps, MpmcRingBuffer, SpscRingBuffer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Queue statistics
#[derive(Debug, Default)]
pub struct QueueStats {
    pub enqueued: AtomicUsize,
    pub dequeued: AtomicUsize,
    pub drops: AtomicUsize,
    pub errors: AtomicUsize,
    pub current_size: AtomicUsize,
    pub peak_size: AtomicUsize,
}

/// Generic ring buffer trait
pub trait RingBuffer<T> {
    /// Push an item to the queue
    fn push(&self, item: T) -> Result<()>;

    /// Pop an item from the queue
    fn pop(&self) -> Result<T>;

    /// Push multiple items in batch
    fn push_batch(&self, items: &[T]) -> Result<()>
    where
        T: Copy;

    /// Pop multiple items in batch
    fn pop_batch(&self, items: &mut [T]) -> Result<usize>
    where
        T: Copy;

    /// Get queue capacity
    fn capacity(&self) -> usize;

    /// Get current size
    fn size(&self) -> usize;

    /// Check if queue is empty
    fn is_empty(&self) -> bool;

    /// Check if queue is full
    fn is_full(&self) -> bool;

    /// Get queue statistics
    fn stats(&self) -> &QueueStats;
}

/// SPSC (Single Producer Single Consumer) queue wrapper
pub struct SpscQueue<T> {
    /// Inner ring buffer
    inner: SpscRingBuffer<T>,
    /// Queue statistics
    stats: QueueStats,
}

impl<T> SpscQueue<T> {
    /// Create a new SPSC queue
    pub fn new(capacity: usize) -> Result<Self> {
        let inner = SpscRingBuffer::new(capacity);

        Ok(Self {
            inner,
            stats: QueueStats::default(),
        })
    }
}

impl<T> RingBuffer<T> for SpscQueue<T> {
    fn push(&self, item: T) -> Result<()> {
        match self.inner.push(item) {
            Ok(_) => {
                self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                let current_size = self.stats.current_size.fetch_add(1, Ordering::Relaxed) + 1;
                self.stats
                    .peak_size
                    .fetch_max(current_size, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => {
                self.stats.drops.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue full".to_string()))
            }
        }
    }

    fn pop(&self) -> Result<T> {
        match self.inner.pop() {
            Ok(item) => {
                self.stats.dequeued.fetch_add(1, Ordering::Relaxed);
                self.stats.current_size.fetch_sub(1, Ordering::Relaxed);
                Ok(item)
            }
            Err(_) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue empty".to_string()))
            }
        }
    }

    fn push_batch(&self, items: &[T]) -> Result<()>
    where
        T: Copy,
    {
        match self.inner.push_batch(items) {
            Ok(_) => {
                let count = items.len();
                self.stats.enqueued.fetch_add(count, Ordering::Relaxed);
                let current_size =
                    self.stats.current_size.fetch_add(count, Ordering::Relaxed) + count;
                self.stats
                    .peak_size
                    .fetch_max(current_size, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => {
                self.stats.drops.fetch_add(items.len(), Ordering::Relaxed);
                Err(Error::QueueError("Queue full".to_string()))
            }
        }
    }
    fn pop_batch(&self, items: &mut [T]) -> Result<usize>
    where
        T: Copy,
    {
        match self.inner.pop_batch(items) {
            Ok(count) => {
                self.stats.dequeued.fetch_add(count, Ordering::Relaxed);
                self.stats.current_size.fetch_sub(count, Ordering::Relaxed);
                Ok(count)
            }
            Err(_) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue empty".to_string()))
            }
        }
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn size(&self) -> usize {
        self.stats.current_size.load(Ordering::Relaxed)
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    fn is_full(&self) -> bool {
        self.size() >= self.capacity()
    }

    fn stats(&self) -> &QueueStats {
        &self.stats
    }
}

/// MPMC (Multi Producer Multi Consumer) queue wrapper
pub struct MpmcQueue<T> {
    /// Inner ring buffer
    inner: MpmcRingBuffer<T>,
    /// Queue statistics
    stats: QueueStats,
}

impl<T> MpmcQueue<T> {
    /// Create a new MPMC queue
    pub fn new(capacity: usize) -> Result<Self> {
        let inner = MpmcRingBuffer::new(capacity);

        Ok(Self {
            inner,
            stats: QueueStats::default(),
        })
    }
}

impl<T> RingBuffer<T> for MpmcQueue<T> {
    fn push(&self, item: T) -> Result<()> {
        match self.inner.push(item) {
            Ok(_) => {
                self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                let current_size = self.stats.current_size.fetch_add(1, Ordering::Relaxed) + 1;
                self.stats
                    .peak_size
                    .fetch_max(current_size, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => {
                self.stats.drops.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue full".to_string()))
            }
        }
    }

    fn pop(&self) -> Result<T> {
        match self.inner.pop() {
            Ok(item) => {
                self.stats.dequeued.fetch_add(1, Ordering::Relaxed);
                self.stats.current_size.fetch_sub(1, Ordering::Relaxed);
                Ok(item)
            }
            Err(_) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue empty".to_string()))
            }
        }
    }

    fn push_batch(&self, items: &[T]) -> Result<()>
    where
        T: Copy,
    {
        match self.inner.push_batch(items) {
            Ok(_) => {
                let count = items.len();
                self.stats.enqueued.fetch_add(count, Ordering::Relaxed);
                let current_size =
                    self.stats.current_size.fetch_add(count, Ordering::Relaxed) + count;
                self.stats
                    .peak_size
                    .fetch_max(current_size, Ordering::Relaxed);
                Ok(())
            }
            Err(_) => {
                self.stats.drops.fetch_add(items.len(), Ordering::Relaxed);
                Err(Error::QueueError("Queue full".to_string()))
            }
        }
    }

    fn pop_batch(&self, items: &mut [T]) -> Result<usize>
    where
        T: Copy,
    {
        match self.inner.pop_batch(items) {
            Ok(count) => {
                self.stats.dequeued.fetch_add(count, Ordering::Relaxed);
                self.stats.current_size.fetch_sub(count, Ordering::Relaxed);
                Ok(count)
            }
            Err(_) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(Error::QueueError("Queue empty".to_string()))
            }
        }
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn size(&self) -> usize {
        self.stats.current_size.load(Ordering::Relaxed)
    }

    fn is_empty(&self) -> bool {
        self.size() == 0
    }

    fn is_full(&self) -> bool {
        self.size() >= self.capacity()
    }

    fn stats(&self) -> &QueueStats {
        &self.stats
    }
}

/// Queue manager for handling multiple queues
pub struct QueueManager {
    /// SPSC queues
    spsc_queues: HashMap<String, Arc<SpscQueue<*mut Mbuf>>>,
    /// MPMC queues
    mpmc_queues: HashMap<String, Arc<MpmcQueue<*mut Mbuf>>>,
    /// Queue statistics
    stats: QueueManagerStats,
}

/// Queue manager statistics
#[derive(Debug, Default)]
pub struct QueueManagerStats {
    pub total_queues: AtomicUsize,
    pub spsc_queues: AtomicUsize,
    pub mpmc_queues: AtomicUsize,
    pub total_enqueued: AtomicUsize,
    pub total_dequeued: AtomicUsize,
    pub total_drops: AtomicUsize,
}

impl QueueManager {
    /// Create a new queue manager
    pub fn new() -> Self {
        Self {
            spsc_queues: HashMap::new(),
            mpmc_queues: HashMap::new(),
            stats: QueueManagerStats::default(),
        }
    }

    /// Create a new SPSC queue
    pub fn create_spsc_queue(
        &mut self,
        name: String,
        capacity: usize,
    ) -> Result<Arc<SpscQueue<*mut Mbuf>>> {
        let queue = Arc::new(SpscQueue::new(capacity)?);
        self.spsc_queues.insert(name.clone(), queue.clone());

        self.stats.total_queues.fetch_add(1, Ordering::Relaxed);
        self.stats.spsc_queues.fetch_add(1, Ordering::Relaxed);

        Ok(queue)
    }

    /// Create a new MPMC queue
    pub fn create_mpmc_queue(
        &mut self,
        name: String,
        capacity: usize,
    ) -> Result<Arc<MpmcQueue<*mut Mbuf>>> {
        let queue = Arc::new(MpmcQueue::new(capacity)?);
        self.mpmc_queues.insert(name.clone(), queue.clone());

        self.stats.total_queues.fetch_add(1, Ordering::Relaxed);
        self.stats.mpmc_queues.fetch_add(1, Ordering::Relaxed);

        Ok(queue)
    }

    /// Get a SPSC queue by name
    pub fn get_spsc_queue(&self, name: &str) -> Option<Arc<SpscQueue<*mut Mbuf>>> {
        self.spsc_queues.get(name).cloned()
    }

    /// Get a MPMC queue by name
    pub fn get_mpmc_queue(&self, name: &str) -> Option<Arc<MpmcQueue<*mut Mbuf>>> {
        self.mpmc_queues.get(name).cloned()
    }

    /// Remove a queue
    pub fn remove_queue(&mut self, name: &str) -> Result<()> {
        if let Some(_) = self.spsc_queues.remove(name) {
            self.stats.total_queues.fetch_sub(1, Ordering::Relaxed);
            self.stats.spsc_queues.fetch_sub(1, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(_) = self.mpmc_queues.remove(name) {
            self.stats.total_queues.fetch_sub(1, Ordering::Relaxed);
            self.stats.mpmc_queues.fetch_sub(1, Ordering::Relaxed);
            return Ok(());
        }

        Err(Error::QueueError(format!("Queue '{}' not found", name)))
    }

    /// Get queue manager statistics
    pub fn stats(&self) -> QueueManagerStatsView {
        let mut total_enqueued = 0;
        let mut total_dequeued = 0;
        let mut total_drops = 0;

        for queue in self.spsc_queues.values() {
            let stats = queue.stats();
            total_enqueued += stats.enqueued.load(Ordering::Relaxed);
            total_dequeued += stats.dequeued.load(Ordering::Relaxed);
            total_drops += stats.drops.load(Ordering::Relaxed);
        }

        for queue in self.mpmc_queues.values() {
            let stats = queue.stats();
            total_enqueued += stats.enqueued.load(Ordering::Relaxed);
            total_dequeued += stats.dequeued.load(Ordering::Relaxed);
            total_drops += stats.drops.load(Ordering::Relaxed);
        }

        QueueManagerStatsView {
            total_queues: self.stats.total_queues.load(Ordering::Relaxed),
            spsc_queues: self.stats.spsc_queues.load(Ordering::Relaxed),
            mpmc_queues: self.stats.mpmc_queues.load(Ordering::Relaxed),
            total_enqueued,
            total_dequeued,
            total_drops,
        }
    }
}

/// Queue manager statistics view
#[derive(Debug)]
pub struct QueueManagerStatsView {
    pub total_queues: usize,
    pub spsc_queues: usize,
    pub mpmc_queues: usize,
    pub total_enqueued: usize,
    pub total_dequeued: usize,
    pub total_drops: usize,
}

/// Worker thread for processing queues
pub struct QueueWorker {
    /// Worker ID
    #[allow(dead_code)]
    id: usize,
    /// Queue to process
    queue: Arc<dyn RingBuffer<*mut Mbuf> + Send + Sync>,
    /// Processing function
    processor: Arc<dyn Fn(*mut Mbuf) -> Result<()> + Send + Sync>,
    /// Running flag
    running: Arc<AtomicBool>,
    /// Worker thread handle
    thread_handle: Option<JoinHandle<Result<()>>>,
    /// Worker statistics
    stats: Arc<WorkerStats>,
}

/// Worker statistics
#[derive(Debug, Default)]
pub struct WorkerStats {
    pub processed: AtomicUsize,
    pub errors: AtomicUsize,
    pub runtime: AtomicUsize, // Runtime in milliseconds
}

impl QueueWorker {
    /// Create a new queue worker
    pub fn new(
        id: usize,
        queue: Arc<dyn RingBuffer<*mut Mbuf> + Send + Sync>,
        processor: Arc<dyn Fn(*mut Mbuf) -> Result<()> + Send + Sync>,
    ) -> Self {
        Self {
            id,
            queue,
            processor,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            stats: Arc::new(WorkerStats::default()),
        }
    }

    /// Start the worker
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);

        let queue = self.queue.clone();
        // Note: We can't clone Fn closures, so we use Arc for sharing
        let processor = std::sync::Arc::clone(&self.processor);
        let running = self.running.clone();
        let stats = Arc::new(std::mem::take(&mut self.stats));

        let thread_handle = thread::spawn(move || -> Result<()> {
            let start_time = std::time::Instant::now();
            let batch_size = 32;
            let mut batch = Vec::with_capacity(batch_size);

            while running.load(Ordering::Relaxed) {
                // Try to pop a batch of items
                batch.clear();
                match queue.pop_batch(&mut batch) {
                    Ok(count) => {
                        if count > 0 {
                            // Process each item
                            for &mbuf in &batch {
                                match processor(mbuf) {
                                    Ok(_) => {
                                        stats.processed.fetch_add(1, Ordering::Relaxed);
                                    }
                                    Err(_) => {
                                        stats.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        } else {
                            // No items available, sleep briefly
                            thread::sleep(Duration::from_micros(10));
                        }
                    }
                    Err(_) => {
                        // Queue empty or error, sleep briefly
                        thread::sleep(Duration::from_micros(10));
                    }
                }
            }

            let runtime = start_time.elapsed().as_millis() as usize;
            stats.runtime.store(runtime, Ordering::Relaxed);

            Ok(())
        });

        self.thread_handle = Some(thread_handle);
        Ok(())
    }

    /// Stop the worker
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);

        if let Some(handle) = self.thread_handle.take() {
            match handle.join() {
                Ok(_) => Ok(()),
                Err(_) => Err(Error::QueueError(
                    "Failed to join worker thread".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }

    /// Get worker statistics
    pub fn stats(&self) -> &WorkerStats {
        &self.stats
    }

    /// Check if worker is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MbufPool, PacketType};

    #[test]
    fn test_spsc_queue() {
        let queue = SpscQueue::<*mut Mbuf>::new(1024).unwrap();

        // Test basic operations
        assert!(queue.is_empty());
        assert!(!queue.is_full());

        // Push and pop
        let mbuf = std::ptr::null_mut();
        queue.push(mbuf).unwrap();
        assert_eq!(queue.size(), 1);

        let popped = queue.pop().unwrap();
        assert_eq!(popped, mbuf);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_mpmc_queue() {
        let queue = MpmcQueue::<*mut Mbuf>::new(1024).unwrap();

        // Test basic operations
        assert!(queue.is_empty());
        assert!(!queue.is_full());

        // Push and pop
        let mbuf = std::ptr::null_mut();
        queue.push(mbuf).unwrap();
        assert_eq!(queue.size(), 1);

        let popped = queue.pop().unwrap();
        assert_eq!(popped, mbuf);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_queue_manager() {
        let mut manager = QueueManager::new();

        // Create queues
        let spsc_queue = manager
            .create_spsc_queue("test_spsc".to_string(), 1024)
            .unwrap();
        let mpmc_queue = manager
            .create_mpmc_queue("test_mpmc".to_string(), 1024)
            .unwrap();

        // Get queues
        let retrieved_spsc = manager.get_spsc_queue("test_spsc").unwrap();
        let retrieved_mpmc = manager.get_mpmc_queue("test_mpmc").unwrap();

        assert!(Arc::ptr_eq(&spsc_queue, &retrieved_spsc));
        assert!(Arc::ptr_eq(&mpmc_queue, &retrieved_mpmc));

        // Check statistics
        let stats = manager.stats();
        assert_eq!(stats.total_queues, 2);
        assert_eq!(stats.spsc_queues, 1);
        assert_eq!(stats.mpmc_queues, 1);
    }

    #[test]
    fn test_batch_operations() {
        let queue = SpscQueue::<*mut Mbuf>::new(1024).unwrap();

        // Test batch push
        let items = vec![std::ptr::null_mut(); 10];
        queue.push_batch(&items).unwrap();
        assert_eq!(queue.size(), 10);

        // Test batch pop
        let mut output = vec![std::ptr::null_mut(); 10];
        let count = queue.pop_batch(&mut output).unwrap();
        assert_eq!(count, 10);
        assert!(queue.is_empty());
    }
}

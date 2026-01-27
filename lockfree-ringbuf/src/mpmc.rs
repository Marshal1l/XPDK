use crate::{BatchOps, Error, RingBufferStorage};
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_utils::Backoff;
use crossbeam_utils::CachePadded;

/// A lock-free Multi Producer Multi Consumer (MPMC) ring buffer
///
/// Multiple threads can push and pop concurrently.
/// Uses atomic operations for coordination between all threads.
pub struct MpmcRingBuffer<T> {
    /// Ring buffer storage
    storage: RingBufferStorage<T>,
    /// Head index (consumer position)
    head: CachePadded<AtomicUsize>,
    /// Tail index (producer position)
    tail: CachePadded<AtomicUsize>,
}

impl<T> MpmcRingBuffer<T> {
    /// Create a new MPMC ring buffer with the given capacity
    /// Capacity will be rounded up to the next power of 2
    pub fn new(capacity: usize) -> Self {
        Self {
            storage: RingBufferStorage::new(capacity),
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
        }
    }

    /// Get the capacity of the ring buffer
    pub fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Try to push a value into the ring buffer
    /// Returns Ok(()) if successful, Err(Error::Full) if the buffer is full
    pub fn push(&self, value: T) -> Result<(), Error> {
        let backoff = Backoff::new();

        loop {
            let tail = self.tail.load(Ordering::Relaxed);
            let head = self.head.load(Ordering::Acquire);

            if tail.wrapping_sub(head) >= self.storage.capacity() {
                return Err(Error::Full);
            }

            // Try to reserve the slot
            if self
                .tail
                .compare_exchange_weak(
                    tail,
                    tail.wrapping_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // Successfully reserved, write the value
                unsafe {
                    self.storage.write(tail, value);
                }
                return Ok(());
            }

            backoff.snooze();
        }
    }

    /// Try to pop a value from the ring buffer
    /// Returns Ok(value) if successful, Err(Error::Empty) if the buffer is empty
    pub fn pop(&self) -> Result<T, Error> {
        let backoff = Backoff::new();

        loop {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);

            if head == tail {
                return Err(Error::Empty);
            }

            // Try to reserve the slot for consumption
            if self
                .head
                .compare_exchange_weak(
                    head,
                    head.wrapping_add(1),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // Successfully reserved, read the value
                let value = unsafe { self.storage.read(head) };
                return Ok(value);
            }

            backoff.snooze();
        }
    }

    /// Check if the ring buffer is empty
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        head == tail
    }

    /// Check if the ring buffer is full
    pub fn is_full(&self) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head) >= self.storage.capacity()
    }

    /// Get the number of items currently in the buffer
    pub fn len(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }
}

impl<T: Copy> BatchOps<T> for MpmcRingBuffer<T> {
    fn push_batch(&self, items: &[T]) -> Result<(), Error> {
        if items.is_empty() {
            return Ok(());
        }

        let backoff = Backoff::new();

        loop {
            let tail = self.tail.load(Ordering::Relaxed);
            let head = self.head.load(Ordering::Acquire);
            let available = self.storage.capacity() - tail.wrapping_sub(head);

            if items.len() > available {
                return Err(Error::Full);
            }

            // Try to reserve the batch slots
            if self
                .tail
                .compare_exchange_weak(
                    tail,
                    tail.wrapping_add(items.len()),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // Successfully reserved, write the batch
                unsafe {
                    self.storage.write_batch(tail, items);
                }
                return Ok(());
            }

            backoff.snooze();
        }
    }

    fn pop_batch(&self, buf: &mut [T]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let backoff = Backoff::new();

        loop {
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);
            let available = tail.wrapping_sub(head);

            if available == 0 {
                return Err(Error::Empty);
            }

            let count = core::cmp::min(buf.len(), available);

            // Try to reserve the batch slots
            if self
                .head
                .compare_exchange_weak(
                    head,
                    head.wrapping_add(count),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // Successfully reserved, read the batch
                unsafe {
                    self.storage.read_batch(head, &mut buf[..count]);
                }
                return Ok(count);
            }

            backoff.snooze();
        }
    }
}

unsafe impl<T: Send> Send for MpmcRingBuffer<T> {}
unsafe impl<T: Sync> Sync for MpmcRingBuffer<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_basic_push_pop() {
        let rb: MpmcRingBuffer<i32> = MpmcRingBuffer::new(4);

        assert!(rb.push(1).is_ok());
        assert!(rb.push(2).is_ok());

        assert_eq!(rb.pop(), Ok(1));
        assert_eq!(rb.pop(), Ok(2));
        assert!(rb.pop().is_err());
    }

    #[test]
    fn test_batch_operations() {
        let rb: MpmcRingBuffer<i32> = MpmcRingBuffer::new(8);
        let items = vec![1, 2, 3, 4];

        assert!(rb.push_batch(&items).is_ok());
        assert_eq!(rb.len(), 4);

        let mut buf = [0; 6];
        let count = rb.pop_batch(&mut buf).unwrap();
        assert_eq!(count, 4);
        assert_eq!(&buf[..4], &[1, 2, 3, 4]);

        assert!(rb.is_empty());
    }
}

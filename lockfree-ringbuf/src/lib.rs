extern crate alloc;

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use crossbeam_utils::CachePadded;

mod mpmc;
mod mpsc;
mod spmc;
mod spsc;

pub use mpmc::MpmcRingBuffer;
pub use mpsc::MpscRingBuffer;
pub use spmc::SpmcRingBuffer;
pub use spsc::SpscRingBuffer;

/// Error types for ring buffer operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The ring buffer is full
    Full,
    /// The ring buffer is empty
    Empty,
}

/// Core ring buffer storage
struct RingBufferStorage<T> {
    /// The buffer storage
    buffer: UnsafeCell<Vec<T>>,
    /// Capacity of the buffer (always a power of 2)
    capacity: usize,
    /// Mask for fast modulo operation (capacity - 1)
    mask: usize,
}

impl<T> RingBufferStorage<T> {
    /// Create a new ring buffer with the given capacity
    /// Capacity will be rounded up to the next power of 2
    fn new(capacity: usize) -> Self {
        let capacity = if capacity.is_power_of_two() {
            capacity
        } else {
            capacity.next_power_of_two()
        };

        let mut buffer = Vec::with_capacity(capacity);
        unsafe {
            buffer.set_len(capacity);
        }

        Self {
            buffer: UnsafeCell::new(buffer),
            capacity,
            mask: capacity - 1,
        }
    }

    /// Get the capacity of the ring buffer
    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the mask for fast modulo operation
    #[inline]
    fn mask(&self) -> usize {
        self.mask
    }

    /// Get a pointer to the buffer
    #[inline]
    fn buffer_ptr(&self) -> *mut T {
        unsafe { (*self.buffer.get()).as_mut_ptr() }
    }

    /// Read a value from the buffer at the given index
    #[inline]
    unsafe fn read(&self, index: usize) -> T {
        let buffer = &*self.buffer.get();
        core::ptr::read(buffer.get_unchecked(index & self.mask))
    }

    /// Write a value to the buffer at the given index
    #[inline]
    unsafe fn write(&self, index: usize, value: T) {
        let buffer = &mut *self.buffer.get();
        core::ptr::write(buffer.get_unchecked_mut(index & self.mask), value);
    }

    /// Read multiple values from the buffer
    unsafe fn read_batch(&self, start_index: usize, dst: &mut [T]) {
        let buffer = &*self.buffer.get();
        let mask = self.mask;

        for (i, dst_item) in dst.iter_mut().enumerate() {
            *dst_item = core::ptr::read(buffer.get_unchecked((start_index + i) & mask));
        }
    }

    /// Write multiple values to the buffer
    unsafe fn write_batch(&self, start_index: usize, src: &[T])
    where
        T: Copy,
    {
        let buffer = &mut *self.buffer.get();
        let mask = self.mask;

        for (i, &src_item) in src.iter().enumerate() {
            core::ptr::write(buffer.get_unchecked_mut((start_index + i) & mask), src_item);
        }
    }
}

unsafe impl<T: Send> Send for RingBufferStorage<T> {}
unsafe impl<T: Sync> Sync for RingBufferStorage<T> {}

impl<T> Drop for RingBufferStorage<T> {
    fn drop(&mut self) {
        // Drop all elements in the buffer
        unsafe {
            let buffer = &mut *self.buffer.get();
            for item in buffer.iter_mut() {
                core::ptr::drop_in_place(item);
            }
        }
    }
}

/// Helper trait for batch operations
pub trait BatchOps<T> {
    /// Push multiple items to the queue
    fn push_batch(&self, items: &[T]) -> Result<(), Error>;

    /// Pop multiple items from the queue
    fn pop_batch(&self, buf: &mut [T]) -> Result<usize, Error>;
}

/// Calculate the next power of 2 greater than or equal to n
#[inline]
fn next_power_of_two(n: usize) -> usize {
    if n.is_power_of_two() {
        n
    } else {
        1 << (64 - n.leading_zeros() as usize)
    }
}

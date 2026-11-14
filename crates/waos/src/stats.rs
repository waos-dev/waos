//! What one program is costing: the numbers an embedding app puts beside it.
//!
//! The engine writes these as the program runs and the app reads them from wherever it shows
//! them, so every number is an atomic and the whole thing is shared as an `Arc`. Nothing here
//! limits a program; that is the runtime's, and what enforces a limit only reports the growth it
//! let through.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// What a program has cost so far, read from any thread.
#[derive(Default)]
pub struct Stats {
    fuel_consumed: AtomicU64,
    linear_memory_bytes: AtomicU64,
    frames_presented: AtomicU64,
    last_present: AtomicU64,
    hardware_buffer_bytes: AtomicU64,
    workers: AtomicU64,
}

impl Stats {
    /// Fuel the program's instructions have burned, as the runtime counts it, in every instance
    /// of its code.
    ///
    /// It is read where wasm is interrupted, so it only moves while the runtime ticks the
    /// program's epoch; an app that never ticks reads zero.
    pub fn fuel_consumed(&self) -> u64 {
        self.fuel_consumed.load(Ordering::Relaxed)
    }

    /// The bytes of linear memory the program holds, over all its memories in every instance of
    /// its code.
    pub fn linear_memory_bytes(&self) -> u64 {
        self.linear_memory_bytes.load(Ordering::Relaxed)
    }

    /// Frames the program has put on screen.
    pub fn frames_presented(&self) -> u64 {
        self.frames_presented.load(Ordering::Relaxed)
    }

    /// When the last of them was presented, on the program's clock.
    pub fn last_present(&self) -> Duration {
        Duration::from_nanos(self.last_present.load(Ordering::Relaxed))
    }

    /// The bytes of the hardware buffers the program's views hold, and of those it holds of its
    /// own, allocated or decoded.
    pub fn hardware_buffer_bytes(&self) -> u64 {
        self.hardware_buffer_bytes.load(Ordering::Relaxed)
    }

    /// The workers the program runs now, its workers' workers among them.
    pub fn workers(&self) -> u64 {
        self.workers.load(Ordering::Relaxed)
    }

    pub(crate) fn worker_started(&self) {
        self.workers.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn worker_ended(&self) {
        self.workers.fetch_sub(1, Ordering::Relaxed);
    }

    /// One instance burned `fuel` more since it was last read.
    pub(crate) fn fuel_burned(&self, fuel: u64) {
        self.fuel_consumed.fetch_add(fuel, Ordering::Relaxed);
    }

    pub(crate) fn memory_grew(&self, bytes: u64) {
        self.linear_memory_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// An instance ended, and its memories, `bytes` of them, with it.
    pub(crate) fn memory_released(&self, bytes: u64) {
        self.linear_memory_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    pub(crate) fn frame_presented(&self, at: u64) {
        self.frames_presented.fetch_add(1, Ordering::Relaxed);
        self.last_present.store(at, Ordering::Relaxed);
    }

    /// One view's hardware buffers, or the program's own, went from `before` bytes to `after`.
    pub(crate) fn hardware_buffers_changed(&self, before: u64, after: u64) {
        if after >= before {
            self.hardware_buffer_bytes
                .fetch_add(after - before, Ordering::Relaxed);
        } else {
            self.hardware_buffer_bytes
                .fetch_sub(before - after, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_present_counts_and_stamps() {
        let stats = Stats::default();
        stats.frame_presented(7);
        stats.frame_presented(9);
        assert_eq!(stats.frames_presented(), 2);
        assert_eq!(stats.last_present(), Duration::from_nanos(9));
    }

    #[test]
    fn hardware_buffers_count_what_each_view_holds_now() {
        let stats = Stats::default();
        stats.hardware_buffers_changed(0, 300);
        stats.hardware_buffers_changed(0, 200);
        stats.hardware_buffers_changed(300, 100);
        assert_eq!(stats.hardware_buffer_bytes(), 300);
    }
}

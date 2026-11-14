//! The bytes of the hardware buffers one program holds of its own, the ones it allocates and the
//! ones the hardware codecs decode for it, counted into its stats beside its views' hardware
//! buffers, which the app puts beside it: counted as each buffer is made and given back once its
//! memory goes, after the program, every texture made of it and every layer showing it have let go
//! of it. They are counted, not bounded: only the system refuses a buffer, with `out-of-memory`. A
//! video frame's buffer is not counted: its decoder's pool bounds it.
//!
//! The invariant: what is counted is given back exactly once, by the [`CountedBytes`] that counted
//! it.

use std::sync::Arc;

use crate::stats::Stats;

/// The bytes one buffer is counted for in its program's stats, given back when this is dropped.
pub(crate) struct CountedBytes {
    stats: Arc<Stats>,
    bytes: u64,
}

impl CountedBytes {
    /// `bytes` more held, counted in `stats` until what is answered is dropped.
    pub(crate) fn new(stats: Arc<Stats>, bytes: u64) -> CountedBytes {
        stats.hardware_buffers_changed(0, bytes);
        CountedBytes { stats, bytes }
    }
}

impl Drop for CountedBytes {
    fn drop(&mut self) {
        self.stats.hardware_buffers_changed(self.bytes, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_counted_are_given_back_as_each_buffer_goes() {
        let stats = Arc::new(Stats::default());
        let first = CountedBytes::new(Arc::clone(&stats), 60);
        let second = CountedBytes::new(Arc::clone(&stats), 40);
        assert_eq!(stats.hardware_buffer_bytes(), 100);
        drop(first);
        assert_eq!(stats.hardware_buffer_bytes(), 40);
        drop(second);
        assert_eq!(stats.hardware_buffer_bytes(), 0);
    }
}

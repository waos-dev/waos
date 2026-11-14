//! What the engine will let one store have, and what it reports of the growth it allowed: the
//! program's stats count every store's memory, and let a store's go when the store does.

use std::sync::Arc;

use wasmtime::{ResourceLimiter, Result, StoreLimits};

use crate::stats::Stats;

/// The store's limits with the growth they allow written into the stats. Every answer is
/// [`StoreLimits`]': what a program may have is decided in one place, and this only watches.
pub(super) struct Limits {
    limits: StoreLimits,
    stats: Arc<Stats>,
    /// The bytes of memory the store's growth has added to the stats.
    counted: u64,
}

impl Limits {
    pub(super) fn new(limits: StoreLimits, stats: Arc<Stats>) -> Limits {
        Limits {
            limits,
            stats,
            counted: 0,
        }
    }
}

impl Drop for Limits {
    /// The store is gone, and its memories with it.
    fn drop(&mut self) {
        self.stats.memory_released(self.counted);
    }
}

impl ResourceLimiter for Limits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        let allowed = self.limits.memory_growing(current, desired, maximum)?;
        if allowed {
            let grown = desired.saturating_sub(current) as u64;
            self.stats.memory_grew(grown);
            self.counted += grown;
        }
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        self.limits.table_growing(current, desired, maximum)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> Result<()> {
        self.limits.memory_grow_failed(error)
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> Result<()> {
        self.limits.table_grow_failed(error)
    }

    fn instances(&self) -> usize {
        self.limits.instances()
    }

    fn tables(&self) -> usize {
        self.limits.tables()
    }

    fn memories(&self) -> usize {
        self.limits.memories()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use wasmtime::StoreLimitsBuilder;

    fn limits(stats: &Arc<Stats>) -> Limits {
        Limits::new(
            StoreLimitsBuilder::new().memory_size(1024).build(),
            Arc::clone(stats),
        )
    }

    #[test]
    fn growth_the_limits_allow_is_counted() {
        let stats = Arc::new(Stats::default());
        let mut limits = limits(&stats);
        assert!(limits.memory_growing(0, 512, None).unwrap());
        assert!(limits.memory_growing(512, 768, None).unwrap());
        assert_eq!(stats.linear_memory_bytes(), 768);
    }

    #[test]
    fn a_stores_memory_is_let_go_with_it() {
        let stats = Arc::new(Stats::default());
        let mut kept = limits(&stats);
        let mut dropped = limits(&stats);
        assert!(kept.memory_growing(0, 256, None).unwrap());
        assert!(dropped.memory_growing(0, 512, None).unwrap());
        drop(dropped);
        assert_eq!(stats.linear_memory_bytes(), 256);
    }

    #[test]
    fn growth_the_limits_refuse_is_not_counted() {
        let stats = Arc::new(Stats::default());
        let mut limits = limits(&stats);
        assert!(!limits.memory_growing(0, 2048, None).unwrap());
        assert_eq!(stats.linear_memory_bytes(), 0);
    }
}

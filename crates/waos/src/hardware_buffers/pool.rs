//! A view's pool of hardware buffers: which the shell holds, and which one is being drawn. Choosing
//! where the next frame goes, and which hardware buffers may go, is kept apart from the GPU so it
//! is tested without one: `T` is the hardware buffer's memory.
//!
//! The invariant: an index names a hardware buffer only while the pool's lock is held, since
//! letting a hardware buffer go moves the ones after it. What must last longer, the hardware buffer
//! being drawn, is marked on the hardware buffer itself.

use std::sync::{Arc, Weak};

pub(super) struct HardwareBufferPool<T> {
    buffers: Vec<PooledHardwareBuffer<T>>,
}

struct PooledHardwareBuffer<T> {
    memory: T,
    /// Live while the shell holds the frame drawn into it.
    held: Weak<Held>,
    /// Handed out for the frame being drawn, and not presented yet.
    being_drawn: bool,
}

/// What the shell's `released` holds. When it goes, called or dropped, the hardware buffer is free,
/// and what follows the hardware buffers hears it.
struct Held {
    released: Box<dyn Fn() + Send + Sync>,
}

impl Drop for Held {
    fn drop(&mut self) {
        (self.released)();
    }
}

/// What the shell is given to call once the compositor is done with a hardware buffer.
pub(crate) type Released = Box<dyn FnOnce() + Send + Sync>;

impl<T> HardwareBufferPool<T> {
    pub(super) fn new(buffers: impl IntoIterator<Item = T>) -> HardwareBufferPool<T> {
        HardwareBufferPool {
            buffers: buffers.into_iter().map(PooledHardwareBuffer::new).collect(),
        }
    }

    pub(super) fn count(&self) -> usize {
        self.buffers.len()
    }

    pub(super) fn memory(&self, index: usize) -> &T {
        &self.buffers[index].memory
    }

    /// A hardware buffer more over `memory`, free and never handed out; its index.
    pub(super) fn add(&mut self, memory: T) -> usize {
        self.buffers.push(PooledHardwareBuffer::new(memory));
        self.count() - 1
    }

    /// Hands `index` out for the frame being drawn, in place of any hardware buffer handed out
    /// before and not presented.
    pub(super) fn hand_out(&mut self, index: usize) {
        for buffer in &mut self.buffers {
            buffer.being_drawn = false;
        }
        self.buffers[index].being_drawn = true;
    }

    /// The hardware buffer being drawn, now presented: marked as the shell's until the `released`
    /// answered is called or dropped, with `freed` run then. `None` when no hardware buffer was
    /// handed out since the last present.
    pub(super) fn present_drawn(
        &mut self,
        freed: impl Fn() + Send + Sync + 'static,
    ) -> Option<(&T, Released)> {
        let buffer = self.buffers.iter_mut().find(|buffer| buffer.being_drawn)?;
        buffer.being_drawn = false;
        let held = Arc::new(Held {
            released: Box::new(freed),
        });
        buffer.held = Arc::downgrade(&held);
        Some((&buffer.memory, Box::new(move || drop(held))))
    }

    /// Lets go of the hardware buffers the shell does not hold and nobody draws into, the last
    /// first, until `keep` are left; answers how many went.
    pub(super) fn let_go_of_unused(&mut self, keep: usize) -> usize {
        let mut let_go = 0;
        let mut index = self.count();
        while index > 0 && self.count() > keep {
            index -= 1;
            if self.buffers[index].is_unused() {
                self.buffers.remove(index);
                let_go += 1;
            }
        }
        let_go
    }

    /// The first hardware buffer the shell does not hold: where the next frame goes; `None` while
    /// the shell holds every one, and then the next frame goes into a hardware buffer added.
    pub(super) fn first_free(&self) -> Option<usize> {
        self.buffers.iter().position(PooledHardwareBuffer::is_free)
    }
}

impl<T> PooledHardwareBuffer<T> {
    fn new(memory: T) -> PooledHardwareBuffer<T> {
        PooledHardwareBuffer {
            memory,
            held: Weak::new(),
            being_drawn: false,
        }
    }

    fn is_free(&self) -> bool {
        self.held.strong_count() == 0
    }

    /// Neither held by the shell nor being drawn.
    fn is_unused(&self) -> bool {
        self.is_free() && !self.being_drawn
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A pool whose hardware buffers are numbered from zero.
    fn numbered(count: usize) -> HardwareBufferPool<usize> {
        HardwareBufferPool::new(0..count)
    }

    /// Hands `index` out and presents it at once; what is answered is the shell's hold on it.
    fn present(pool: &mut HardwareBufferPool<usize>, index: usize) -> Released {
        pool.hand_out(index);
        let (_, released) = pool
            .present_drawn(|| {})
            .expect("a hardware buffer was drawn");
        released
    }

    fn memories(pool: &HardwareBufferPool<usize>) -> Vec<usize> {
        (0..pool.count()).map(|index| *pool.memory(index)).collect()
    }

    #[test]
    fn a_hardware_buffer_is_held_until_the_shell_releases_it() {
        let mut pool = numbered(3);
        let released = present(&mut pool, 0);
        assert_eq!(pool.first_free(), Some(1));
        released();
        assert_eq!(pool.first_free(), Some(0));
    }

    #[test]
    fn a_released_let_go_of_uncalled_frees_the_hardware_buffer_too() {
        let mut pool = numbered(3);
        let released = present(&mut pool, 0);
        drop(released);
        assert_eq!(pool.first_free(), Some(0), "the shell is done with it");
    }

    #[test]
    fn a_freed_hardware_buffer_is_announced_once_it_is_released() {
        let freed = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&freed);
        let mut pool = numbered(3);
        pool.hand_out(0);
        let (_, released) = pool
            .present_drawn(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert_eq!(freed.load(Ordering::SeqCst), 0);
        released();
        assert_eq!(freed.load(Ordering::SeqCst), 1);
        assert_eq!(
            pool.first_free(),
            Some(0),
            "free by the time it is announced"
        );
    }

    #[test]
    fn a_frame_goes_into_a_free_hardware_buffer_or_one_added_however_many_the_shell_holds() {
        let mut pool = numbered(3);
        let mut held: Vec<Released> = Vec::new();
        for index in 0..8 {
            let free = pool.first_free().unwrap_or_else(|| pool.add(index));
            assert_eq!(free, index, "every hardware buffer before it is held");
            held.push(present(&mut pool, free));
        }
        assert_eq!(pool.count(), 8, "the pool grew as far as the shell held");
        held.remove(2)();
        assert_eq!(
            pool.first_free(),
            Some(2),
            "a released hardware buffer is drawn into again"
        );
    }

    #[test]
    fn only_what_is_handed_out_since_the_last_present_is_presented() {
        let mut pool = numbered(3);
        assert!(pool.present_drawn(|| {}).is_none(), "nothing was drawn");
        pool.hand_out(0);
        pool.hand_out(1);
        let (memory, _released) = pool.present_drawn(|| {}).unwrap();
        assert_eq!(
            *memory, 1,
            "the hardware buffer handed out last is the one drawn"
        );
        assert!(pool.present_drawn(|| {}).is_none(), "presented once");
    }

    #[test]
    fn unused_hardware_buffers_go_the_last_first_down_to_those_kept() {
        let mut pool = numbered(5);
        assert_eq!(pool.let_go_of_unused(3), 2);
        assert_eq!(memories(&pool), [0, 1, 2]);
        assert_eq!(pool.let_go_of_unused(3), 0, "three are kept");
    }

    #[test]
    fn a_hardware_buffer_held_or_being_drawn_stays_wherever_it_is() {
        let mut pool = numbered(4);
        let _on_screen = present(&mut pool, 1);
        pool.hand_out(2);
        assert_eq!(pool.let_go_of_unused(0), 2);
        assert_eq!(memories(&pool), [1, 2]);
        let (drawn, _presented) = pool.present_drawn(|| {}).unwrap();
        assert_eq!(
            *drawn, 2,
            "the hardware buffer being drawn is still marked so"
        );
    }
}

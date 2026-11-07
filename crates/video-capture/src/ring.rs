//! Pre-roll replay ring buffer.
//!
//! Frames arrive as `(timestamp_100ns_ticks, BGRA bytes)` pairs: ticks are the
//! 100 ns unit the Windows capture API timestamps frames in, so the engine
//! feeds frames in without owning a clock here. The ring owns its buffers —
//! no `Clone` (single-owner resource), no `unsafe`.

use std::collections::VecDeque;

/// Capacity-bounded ring of pre-roll frames; the oldest slot is evicted when the
/// ring grows past capacity, so it always holds the newest `max_frames`. The
/// capacity can change at runtime via [`ReplayRing::set_capacity`].
pub struct ReplayRing {
    /// Oldest slot first, newest last — snapshot order is replay order.
    slots: VecDeque<(i64, Vec<u8>)>,
    max_frames: usize,
}

impl ReplayRing {
    /// Ring holding at most `max_frames` frames; `max_frames == 0` stores
    /// nothing and [`ReplayRing::push`] is a no-op.
    pub fn new(max_frames: usize) -> Self {
        Self {
            slots: VecDeque::new(),
            max_frames,
        }
    }

    /// Insert a frame as the newest slot, evicting oldest while over capacity.
    pub fn push(&mut self, timestamp_ticks: i64, bytes: Vec<u8>) {
        if self.max_frames == 0 {
            return;
        }
        self.slots.push_back((timestamp_ticks, bytes));
        while self.slots.len() > self.max_frames {
            self.slots.pop_front();
        }
    }

    /// Change the capacity at runtime.
    ///
    /// Growing keeps every buffered frame (they become the newest window);
    /// shrinking evicts oldest frames until `len <= max_frames`. `max_frames ==
    /// 0` drains the ring and releases the frame buffers and the deque's
    /// backing allocation, so [`ReplayRing::push`] stores nothing afterwards.
    pub fn set_capacity(&mut self, max_frames: usize) {
        self.max_frames = max_frames;
        if max_frames == 0 {
            // Assigning an empty deque drops the frames and frees the backing
            // buffer; `clear()` alone would keep the allocation.
            self.slots = VecDeque::new();
            return;
        }
        while self.slots.len() > max_frames {
            self.slots.pop_front();
        }
    }

    /// Number of buffered frames.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the ring holds no frames.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Buffered frames, oldest first — the order replay must write them.
    pub fn snapshot(&self) -> Vec<(i64, Vec<u8>)> {
        self.slots.iter().cloned().collect()
    }

    /// Drain every frame (recording start replays them and frees the memory).
    pub fn take(&mut self) -> Vec<(i64, Vec<u8>)> {
        self.slots.drain(..).collect()
    }
}

/// Frame slots needed for `seconds` of pre-roll at `fps`, saturating.
pub fn frames_for(seconds: u32, fps: u32) -> usize {
    seconds.saturating_mul(fps) as usize
}

/// BGRA frame bytes for a size (4 bytes per pixel), saturating.
///
/// `u32 -> usize` is exact on every supported target (32-bit `usize` holds
/// `u32::MAX` exactly, 64-bit widens losslessly), so the cast never truncates.
pub fn frame_bytes(width: u32, height: u32) -> usize {
    let pixels = (width as usize).saturating_mul(height as usize);
    pixels.saturating_mul(4)
}

/// Rough memory cost of a `seconds`-long replay window at `fps` and size,
/// saturating so an extreme request never overflows `usize`.
pub fn estimated_memory_bytes(seconds: u32, fps: u32, width: u32, height: u32) -> usize {
    frames_for(seconds, fps).saturating_mul(frame_bytes(width, height))
}

/// Milliseconds to the 100 ns tick unit the capture API timestamps frames in,
/// saturating at the `i64` extremes.
pub fn ticks_from_ms(ms: i64) -> i64 {
    ms.saturating_mul(10_000)
}

#[cfg(test)]
#[path = "ring.tests.rs"]
mod tests;

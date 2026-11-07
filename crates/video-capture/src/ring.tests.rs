//! Unit tests for `ring`: capacity, eviction, ordering, drain, sizing math.
//! Deterministic — timestamps and byte buffers are injected; nothing real.

use super::*;

#[test]
fn ring_zero_capacity_push_is_noop() {
    let mut ring = ReplayRing::new(0);
    ring.push(10, vec![1, 2, 3]);
    assert!(ring.is_empty());
    assert_eq!(ring.len(), 0);
    assert!(ring.snapshot().is_empty());
}

#[test]
fn ring_eviction_keeps_newest() {
    let mut ring = ReplayRing::new(2);
    ring.push(1, vec![1]);
    ring.push(2, vec![2]);
    ring.push(3, vec![3]);
    assert_eq!(ring.len(), 2);
    assert_eq!(ring.snapshot(), vec![(2, vec![2]), (3, vec![3])]);
}

#[test]
fn ring_snapshot_is_oldest_first() {
    let mut ring = ReplayRing::new(3);
    ring.push(100, vec![1]);
    ring.push(200, vec![2]);
    ring.push(300, vec![3]);
    assert_eq!(
        ring.snapshot(),
        vec![(100, vec![1]), (200, vec![2]), (300, vec![3])]
    );
}

#[test]
fn ring_take_drains_and_empties() {
    let mut ring = ReplayRing::new(3);
    ring.push(100, vec![1]);
    ring.push(200, vec![2]);
    let drained = ring.take();
    assert_eq!(drained, vec![(100, vec![1]), (200, vec![2])]);
    assert!(ring.is_empty());
    assert!(ring.snapshot().is_empty());
}

#[test]
fn ring_set_capacity_zero_drains_and_frees_buffered_frames() {
    let mut ring = ReplayRing::new(4);
    ring.push(10, vec![1, 2, 3]);
    ring.push(20, vec![4, 5, 6]);
    ring.set_capacity(0);
    assert_eq!(ring.len(), 0);
    assert!(ring.is_empty());
    assert!(ring.snapshot().is_empty());
    // Disabled ring keeps storing nothing, as `new(0)` does.
    ring.push(30, vec![7, 8, 9]);
    assert_eq!(ring.len(), 0);
    assert!(ring.snapshot().is_empty());
}

#[test]
fn ring_set_capacity_shrink_evicts_oldest_frames() {
    let mut ring = ReplayRing::new(5);
    for frame in 1..=5 {
        ring.push(frame * 10, vec![frame as u8]);
    }
    ring.set_capacity(2);
    assert_eq!(ring.len(), 2);
    // Newest two survive, still oldest-first.
    assert_eq!(ring.snapshot(), vec![(40, vec![4]), (50, vec![5])]);
}

#[test]
fn ring_set_capacity_growth_keeps_existing_frames() {
    let mut ring = ReplayRing::new(3);
    ring.push(100, vec![1]);
    ring.push(200, vec![2]);
    ring.push(300, vec![3]);
    ring.set_capacity(6);
    assert_eq!(ring.len(), 3);
    assert_eq!(
        ring.snapshot(),
        vec![(100, vec![1]), (200, vec![2]), (300, vec![3])]
    );
    // Fill up to the new limit: no eviction until it is exceeded.
    ring.push(400, vec![4]);
    ring.push(500, vec![5]);
    ring.push(600, vec![6]);
    assert_eq!(ring.len(), 6);
    assert_eq!(
        ring.snapshot(),
        vec![
            (100, vec![1]),
            (200, vec![2]),
            (300, vec![3]),
            (400, vec![4]),
            (500, vec![5]),
            (600, vec![6])
        ]
    );
    ring.push(700, vec![7]);
    assert_eq!(ring.len(), 6);
    assert_eq!(
        ring.snapshot(),
        vec![
            (200, vec![2]),
            (300, vec![3]),
            (400, vec![4]),
            (500, vec![5]),
            (600, vec![6]),
            (700, vec![7])
        ]
    );
}

#[test]
fn ring_set_capacity_reenable_after_disable_stores_new_frames() {
    let mut ring = ReplayRing::new(4);
    ring.push(10, vec![1]);
    ring.push(20, vec![2]);
    ring.set_capacity(0);
    assert!(ring.is_empty());
    ring.set_capacity(3);
    assert_eq!(ring.len(), 0);
    for frame in 1..=4 {
        ring.push(frame, vec![frame as u8]);
    }
    assert_eq!(ring.len(), 3);
    assert_eq!(
        ring.snapshot(),
        vec![(2, vec![2]), (3, vec![3]), (4, vec![4])]
    );
}

#[test]
fn frames_for_multiplies_seconds_and_fps() {
    assert_eq!(frames_for(0, 30), 0);
    assert_eq!(frames_for(600, 60), 36_000);
}

#[test]
fn frames_for_extreme_saturates_at_u32_max() {
    assert_eq!(frames_for(u32::MAX, u32::MAX), u32::MAX as usize);
}

#[test]
fn frame_bytes_bgra_is_width_times_height_times_4() {
    assert_eq!(frame_bytes(1920, 1080), 8_294_400);
    assert_eq!(frame_bytes(0, 0), 0);
}

#[test]
fn estimated_memory_600s_60fps_1080p_is_exact_without_overflow() {
    // 36_000 frames * 8_294_400 bytes = 298_598_400_000, fits in 64-bit usize.
    assert_eq!(estimated_memory_bytes(600, 60, 1920, 1080), 298_598_400_000);
}

#[test]
fn estimated_memory_extremes_saturate() {
    assert_eq!(
        estimated_memory_bytes(u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        usize::MAX
    );
    assert_eq!(estimated_memory_bytes(1, 1, u32::MAX, u32::MAX), usize::MAX);
}

#[test]
fn ticks_from_ms_converts_to_100ns_ticks() {
    assert_eq!(ticks_from_ms(0), 0);
    assert_eq!(ticks_from_ms(1), 10_000);
    assert_eq!(ticks_from_ms(100), 1_000_000);
}

#[test]
fn ticks_from_ms_negative_maps_to_negative_ticks() {
    assert_eq!(ticks_from_ms(-5), -50_000);
}

#[test]
fn ticks_from_ms_extremes_saturate() {
    assert_eq!(ticks_from_ms(i64::MAX), i64::MAX);
    assert_eq!(ticks_from_ms(i64::MIN), i64::MIN);
}

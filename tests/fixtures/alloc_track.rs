//! Counting wrapper over the system allocator, feeding the memory
//! columns. Tracks live bytes and a resettable peak; installed for the
//! whole test binary so worker-thread allocations count too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

pub struct Counting;

/// Off until a measurement enables it, so a test that never opts in
/// pays one relaxed load per allocation and no read-write atomics.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Signed because counting can start mid-process: a free of memory
/// allocated before the switch drives the raw figure below zero,
/// and the measurements only ever read differences.
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

pub fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

pub fn peak() -> isize {
    PEAK.load(Ordering::Relaxed)
}

pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

fn grow(bytes: usize) {
    let live = LIVE.fetch_add(bytes as isize, Ordering::Relaxed) + bytes as isize;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn shrink(bytes: usize) {
    LIVE.fetch_sub(bytes as isize, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        if ENABLED.load(Ordering::Relaxed) {
            shrink(layout.size());
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !ptr.is_null() && ENABLED.load(Ordering::Relaxed) {
            if new_size >= layout.size() {
                grow(new_size - layout.size());
            } else {
                shrink(layout.size() - new_size);
            }
        }
        ptr
    }
}

//! Memory validation through a counting allocator. Its own binary,
//! separate from the timing tiers: the wrapper and its atomics tax
//! every allocation, measured at up to a fifth of an allocation-heavy
//! pass, so nothing in here is ever timed. Run release mode locally
//! with:
//!   cargo test --release --test perf_mem -- --ignored --nocapture --test-threads=1

use oryx::style::fonts::FontStore;

#[path = "fixtures/large_gen.rs"]
mod large_gen;

#[path = "fixtures/perf_common.rs"]
mod perf_common;

use perf_common::{
    measure_export, measure_highlight, measure_layout, measure_open, pool, settle_recolor, TIERS,
};

/// Counting wrapper over the system allocator, feeding the memory
/// columns. Tracks live bytes and a resettable peak; installed for the
/// whole test binary so worker-thread allocations count too.
mod alloc_track {
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
}

#[global_allocator]
static ALLOC: alloc_track::Counting = alloc_track::Counting;

fn mb(bytes: isize) -> f32 {
    bytes.max(0) as f32 / (1024.0 * 1024.0)
}

/// The arithmetic the memory columns rest on. Exact equality holds
/// because this test's thread allocates alone between the probes; the
/// ignored measurement tests never run beside it. Counting is off until
/// a measurement enables it; the gate is asserted here first.
#[test]
fn allocator_tracks_live_and_peak() {
    let idle = alloc_track::live();
    let parked: Vec<u8> = Vec::with_capacity(1 << 16);
    assert_eq!(
        alloc_track::live(),
        idle,
        "counting stays off until a measurement enables it"
    );
    drop(parked);

    alloc_track::set_enabled(true);
    alloc_track::reset_peak();
    let base = alloc_track::live();
    assert!(alloc_track::peak() >= base, "peak never sits under live");

    let mut v: Vec<u8> = Vec::with_capacity(1 << 20);
    assert_eq!(
        alloc_track::live(),
        base + (1 << 20),
        "an allocation moves live by its exact size"
    );
    assert!(
        alloc_track::peak() >= base + (1 << 20),
        "peak follows live up"
    );

    v.reserve_exact(1 << 21);
    let grown = alloc_track::live();
    assert_eq!(
        grown,
        base + v.capacity() as isize,
        "a realloc counts the delta against the new capacity"
    );

    drop(v);
    assert_eq!(alloc_track::live(), base, "a free returns its bytes");
    assert!(
        alloc_track::peak() >= grown,
        "peak is monotone and survives the free"
    );

    alloc_track::reset_peak();
    assert_eq!(
        alloc_track::peak(),
        alloc_track::live(),
        "a reset pins peak to live"
    );
    alloc_track::set_enabled(false);
}

/// The memory tiers, counted by the allocator with nothing timed, so
/// the counting overhead costs the columns nothing. Each row walks the
/// app's journey: open, the streamed layout, the highlight fold, the
/// recolor to the settled state, then the export. Live at settle is
/// what the app holds; the peak covers open to settle; the export peak
/// is what an export adds above the settled state. Fixture source and
/// font store are allocated before the base is taken, so the columns
/// cover what the document costs, not the harness.
#[test]
#[ignore = "measurement only"]
fn memory_measured() {
    alloc_track::set_enabled(true);
    let pool = pool();
    for (name, bytes) in TIERS {
        for (kind, ext) in [("md", "md"), ("code", "rs")] {
            let source = if kind == "md" {
                large_gen::generate(*bytes)
            } else {
                large_gen::generate_code(*bytes)
            };
            let mut fonts = FontStore::new();
            alloc_track::reset_peak();
            let base = alloc_track::live();
            let (_, _, mut doc) = measure_open(&source, ext);
            let (_, mut resident) = measure_layout(&doc, Some(&pool));
            measure_highlight(&mut doc);
            settle_recolor(&doc, &mut resident, &mut fonts);
            let settled = alloc_track::live();
            let open_peak = alloc_track::peak();
            alloc_track::reset_peak();
            let (_, _, pdf_bytes) = measure_export(&doc, Some(&pool));
            let export_peak = alloc_track::peak();
            println!(
                "mem {kind:<4} {tier:>6}: settled {:>7.1}MB (peak {:>7.1}MB), \
                 export peak +{:>6.1}MB (pdf {:.1}MB), runs {:>7}",
                mb(settled - base),
                mb(open_peak - base),
                mb(export_peak - settled),
                pdf_bytes as f32 / (1024.0 * 1024.0),
                resident.runs.len(),
                tier = name,
            );
        }
    }
    alloc_track::set_enabled(false);
}

/// What the seam table costs: the app-side (line, parser state) pairs
/// the exact sweep records, one per chunk, which the fixture rows above
/// never see because they hold no app. Measured on the 8MB code tier,
/// the largest table Oryx builds.
#[test]
#[ignore = "measurement only"]
fn seam_table_measured() {
    use oryx::doc::model::BlockKind;
    use oryx::style::highlight::{self, Seam, CHUNK_LINES};
    let source = large_gen::generate_code(8 * 1024 * 1024);
    let (_, _, doc) = measure_open(&source, "rs");
    let (language, lines) = match &doc.blocks[0].kind {
        BlockKind::CodeBlock {
            language, lines, ..
        } => (language.clone(), lines.clone()),
        _ => panic!("a code file is one code block"),
    };
    // One sweep before any measurement: the grammar's regex caches
    // compile lazily inside the first parse and stay live, and
    // counting them against the table reads as megabytes of seams
    // that do not exist.
    highlight::spans_chunked(
        &doc.source,
        &lines,
        language.as_deref(),
        CHUNK_LINES,
        None,
        |_| true,
    );
    alloc_track::set_enabled(true);
    let base = alloc_track::live();
    let mut table: Vec<(usize, Seam)> = Vec::new();
    highlight::spans_chunked(
        &doc.source,
        &lines,
        language.as_deref(),
        CHUNK_LINES,
        None,
        |c| {
            highlight::record_seam(&mut table, c.start_line + c.spans.len(), &c.seam);
            true
        },
    );
    let held = alloc_track::live() - base;
    println!(
        "mem seam table 8MB code: {} seams over {} lines, {:.2}MB held ({} bytes each)",
        table.len(),
        lines.len(),
        mb(held),
        held / table.len().max(1) as isize,
    );
    alloc_track::set_enabled(false);
}

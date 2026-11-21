//! The memory of the layout of a huge paragraph, counted by the
//! allocator of `perf_mem.rs`. Its own binary: the allocator test of
//! `perf_mem.rs` counts every thread exactly, and a test that ends
//! beside it adds the harness's allocations to its count (Task 428).

use oryx::style::fonts::FontStore;

#[path = "fixtures/alloc_track.rs"]
mod alloc_track;

#[global_allocator]
static ALLOC: alloc_track::Counting = alloc_track::Counting;

fn mb(bytes: isize) -> f32 {
    bytes.max(0) as f32 / (1024.0 * 1024.0)
}

/// A plain paragraph is one piece of text, and cosmic-text keeps about
/// 340 bytes per byte of a buffer while it shapes it. A paragraph of
/// 300 KB is shaped in pieces, so the peak is a piece's (Task 428).
#[test]
fn a_huge_paragraph_is_laid_out_in_bounded_memory() {
    let line = format!("{}\n", ["word"; 16].join(" "));
    let source = line.repeat(300_000 / line.len());
    let doc = oryx::doc::markdown::parse(source.as_str());
    let mut fonts = FontStore::new();
    let mut media = oryx::doc::images::MediaCache::offline(std::path::PathBuf::from("."));
    let theme = oryx::style::theme::Theme::default_dark();
    let cfg = oryx::layout::ViewConfig::default();
    alloc_track::set_enabled(true);
    alloc_track::reset_peak();
    let base = alloc_track::live();
    let l = oryx::layout::layout(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0);
    let peak = alloc_track::peak() - base;
    alloc_track::set_enabled(false);
    assert!(l.runs.len() > 1000, "{} runs", l.runs.len());
    assert!(mb(peak) < 40.0, "the layout peaked at {:.1} MB", mb(peak));
}

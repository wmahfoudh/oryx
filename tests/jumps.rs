//! The jump-back data path: what Alt+Left computes when it pushes and
//! pops positions, over a real footnote-heavy markdown file.

use std::path::PathBuf;

use oryx::doc::images::MediaCache;
use oryx::doc::load;
use oryx::doc::model::Document;
use oryx::layout::{layout, LayoutDoc, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

fn footnotes_doc() -> (Document, LayoutDoc) {
    let path = PathBuf::from("tests/showcase/footnotes.md");
    let opened = load::open(&path, None).unwrap();
    let doc = opened.document;
    let mut media = MediaCache::new(PathBuf::from("tests/showcase"));
    let mut fonts = FontStore::new();
    let lay = layout(
        &doc,
        &Theme::default_dark(),
        &mut fonts,
        &mut media,
        &ViewConfig::default(),
        900.0,
    );
    (doc, lay)
}

/// The offset the app pushes when a jump leaves scroll position `y`:
/// the last block whose recorded top is at or above the viewport top.
fn pushed_offset(doc: &Document, lay: &LayoutDoc, scroll_y: f32) -> usize {
    let mut offset = 0usize;
    for (index, block) in doc.blocks.iter().enumerate() {
        match lay.approx_top(index, 0) {
            Some(top) if top <= scroll_y + 1.0 => offset = block.range.start,
            _ => break,
        }
    }
    offset
}

/// The position the pop lands on, as the pending-offset path resolves
/// it; None is the silent drop the reader experiences as a dead key.
fn landing(doc: &Document, lay: &LayoutDoc, offset: usize) -> Option<f32> {
    doc.block_at_offset(offset)
        .and_then(|block| lay.approx_top(block, 0))
}

#[test]
fn a_return_from_mid_document_lands_where_the_reader_was() {
    let (doc, lay) = footnotes_doc();
    assert!(
        lay.anchor_y("footnote:why").is_some(),
        "the footnote jump itself resolves"
    );
    let offset = pushed_offset(&doc, &lay, 600.0);
    let back = landing(&doc, &lay, offset).expect("the return resolves");
    assert!(
        (back - 600.0).abs() < 400.0,
        "the return lands near the reader's position: {back}"
    );
}

/// Clicking a footnote before scrolling pushes the document's head.
/// A markdown heading's range starts past its marker, so the head
/// offset precedes every block; the return must land at the top
/// instead of resolving to nothing.
#[test]
fn a_return_from_the_top_of_a_markdown_file_lands_at_the_top() {
    let (doc, lay) = footnotes_doc();
    let offset = pushed_offset(&doc, &lay, 0.0);
    let back = landing(&doc, &lay, offset).expect("the return from the top resolves");
    assert!(back < 100.0, "the return lands at the top: {back}");
}

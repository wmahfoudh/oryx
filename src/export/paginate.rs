//! Cutting one tall column into pages. Pure: layout and page geometry in,
//! a list of pages out, with no fonts and no PDF anywhere near it.

use std::collections::HashMap;
use std::ops::Range;

use crate::doc::model::{BlockKind, Document};
use crate::export::PageGeometry;
use crate::layout::{metrics, DecoRect, ImagePlace, LayoutDoc};

/// Float slack for comparing positions that arithmetic should have made
/// equal.
const SLACK: f32 = 0.01;

/// One page: where it starts in document coordinates, the runs and images
/// it carries, and its rectangles already split at the break.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub top: f32,
    /// Where this page's content ends. Not the next page's top: a code
    /// panel continued overleaf keeps its inner padding at both edges, so
    /// the two pages each carry a little of the same rectangle.
    pub bottom: f32,
    pub runs: Range<usize>,
    pub images: Vec<ImagePlace>,
    pub rects: Vec<DecoRect>,
}

/// One thing a page can hold whole: a visual line, an image, or a line
/// with an image sitting in it. Pieces that overlap vertically belong to
/// the same item, which keeps a raised footnote reference with the words
/// it follows, an inline badge with its sentence, and a table row's cells
/// together. An item is atomic, so nothing here is ever cut in half.
struct Item {
    runs: Range<usize>,
    images: Vec<usize>,
    top: f32,
    bottom: f32,
}

impl Item {
    /// The block an item belongs to, absent for an image standing alone.
    fn block(&self, layout: &LayoutDoc) -> Option<usize> {
        (!self.runs.is_empty()).then(|| layout.runs[self.runs.start].block)
    }
}

/// Every run and every image, in document order, grouped into items.
fn items(layout: &LayoutDoc) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let (mut run, mut image) = (0, 0);
    loop {
        let run_top = layout.runs.get(run).map(|r| r.y);
        let image_top = layout.images.get(image).map(|i| i.y);
        let take_run = match (run_top, image_top) {
            (Some(a), Some(b)) => a <= b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_run {
            let piece = &layout.runs[run];
            let bottom = piece.y + metrics::LINE_HEIGHT * piece.size;
            match out.last_mut() {
                Some(item) if piece.y < item.bottom - SLACK => {
                    item.runs.end = run + 1;
                    item.top = item.top.min(piece.y);
                    item.bottom = item.bottom.max(bottom);
                }
                _ => out.push(Item {
                    runs: run..run + 1,
                    images: Vec::new(),
                    top: piece.y,
                    bottom,
                }),
            }
            run += 1;
        } else {
            let piece = &layout.images[image];
            let bottom = piece.y + piece.height;
            match out.last_mut() {
                Some(item) if piece.y < item.bottom - SLACK => {
                    item.images.push(image);
                    item.top = item.top.min(piece.y);
                    item.bottom = item.bottom.max(bottom);
                }
                _ => out.push(Item {
                    runs: run..run,
                    images: vec![image],
                    top: piece.y,
                    bottom,
                }),
            }
            image += 1;
        }
    }
    out
}

/// Where each block's items begin and end, so the widow rule costs a
/// lookup rather than a scan.
fn block_spans(layout: &LayoutDoc, items: &[Item]) -> HashMap<usize, (usize, usize)> {
    let mut spans: HashMap<usize, (usize, usize)> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let Some(block) = item.block(layout) else {
            continue;
        };
        spans
            .entry(block)
            .and_modify(|span| span.1 = index)
            .or_insert((index, index));
    }
    spans
}

/// Whether a break before item `k` is one of the ones that read badly.
/// The page currently starts at item `i`.
fn rejected(
    doc: &Document,
    layout: &LayoutDoc,
    items: &[Item],
    spans: &HashMap<usize, (usize, usize)>,
    i: usize,
    k: usize,
) -> bool {
    let (Some(previous), Some(next)) = (items[k - 1].block(layout), items[k].block(layout)) else {
        return false;
    };
    // A heading belongs with what it introduces.
    if matches!(doc.blocks[previous].kind, BlockKind::Heading { .. }) {
        return true;
    }
    // A block that splits leaves at least two lines on each side, which
    // means a block of three or fewer lines never splits at all.
    if previous == next {
        if let Some((first, last)) = spans.get(&previous) {
            let here = k - (*first).max(i);
            let there = last + 1 - k;
            if here < 2 || there < 2 {
                return true;
            }
        }
    }
    false
}

/// Where a page starts, given the first item it carries. Decoration that
/// sits above that item and belongs to it comes too: a continued code
/// panel keeps its inner padding, and a table row starts at its band so
/// the stripe is not left behind on the page before.
fn page_top(doc: &Document, layout: &LayoutDoc, items: &[Item], i: usize) -> f32 {
    let item = &items[i];
    if let Some(block) = item.block(layout) {
        let continued = i > 0 && items[i - 1].block(layout) == Some(block);
        if continued && matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. }) {
            return item.top - metrics::CODE_PAD;
        }
    }
    let row = layout
        .table_rows
        .partition_point(|band| band.top <= item.top + SLACK)
        .saturating_sub(1);
    let mut top = item.top;
    if let Some(band) = layout.table_rows.get(row) {
        if item.top > band.top && item.top < band.bottom {
            top = band.top;
        }
    }
    if i == 0 {
        // Everything above the first item belongs to the first page: a
        // document can open on a panel whose padding sits above its text,
        // and trimming to the item would push it off the sheet.
        top = layout
            .rects
            .iter()
            .map(|rect| rect.y)
            .filter(|y| *y < top)
            .fold(top, f32::min);
    }
    top
}

/// Cuts the column into pages. A page holds as many whole items as its
/// content box takes, and starts flush on the first of them, so the
/// layout's own vertical margin never doubles with the page's.
pub fn paginate(doc: &Document, layout: &LayoutDoc, geometry: &PageGeometry) -> Vec<Page> {
    let content = geometry.content_height();
    let items = items(layout);
    let spans = block_spans(layout, &items);
    let mut pages: Vec<Page> = Vec::new();
    let mut i = 0;
    let mut cursor = 0;
    while i < items.len() {
        let top = page_top(doc, layout, &items, i);
        let mut j = i;
        while j < items.len() && items[j].bottom - top <= content + SLACK {
            j += 1;
        }
        // An item taller than the whole content box is placed anyway. It
        // overflows, and the pass moves on instead of stalling here.
        if j == i {
            j = i + 1;
        }
        if j < items.len() {
            let mut candidate = j;
            loop {
                if !rejected(doc, layout, &items, &spans, i, candidate) {
                    j = candidate;
                    break;
                }
                // Nothing on this page is an acceptable break, so the
                // natural one stands rather than the page emptying.
                if candidate <= i + 1 {
                    break;
                }
                candidate -= 1;
            }
        }
        let end = items[j - 1].runs.end.max(cursor);
        let carried: Vec<usize> = items[i..j]
            .iter()
            .flat_map(|item| item.images.clone())
            .collect();
        pages.push(Page {
            top,
            bottom: f32::INFINITY,
            runs: cursor..end,
            images: carried
                .iter()
                .map(|index| scaled(&layout.images[*index], content))
                .collect(),
            rects: Vec::new(),
        });
        cursor = end;
        i = j;
    }
    if pages.is_empty() {
        pages.push(Page {
            top: 0.0,
            bottom: f32::INFINITY,
            runs: 0..0,
            images: Vec::new(),
            rects: Vec::new(),
        });
    }
    close_pages(doc, layout, &items, &mut pages);
    place_rects(layout, &mut pages);
    pages
}

/// An image taller than a whole page cannot be moved anywhere useful, so
/// it is scaled to the content box and keeps its aspect.
fn scaled(image: &ImagePlace, content: f32) -> ImagePlace {
    let mut placed = image.clone();
    if placed.height > content {
        let scale = content / placed.height;
        placed.width *= scale;
        placed.height *= scale;
    }
    placed
}

/// Sets where each page's content ends. A page normally ends where the
/// next begins, but a code panel carrying on overleaf keeps its padding
/// below its last line, which is one padding past that point.
fn close_pages(doc: &Document, layout: &LayoutDoc, items: &[Item], pages: &mut [Page]) {
    let mut at = 0;
    for index in 0..pages.len().saturating_sub(1) {
        let next_top = pages[index + 1].top;
        // The last item of this page is the one before the first item of
        // the next, which starts at the next page's own top.
        let mut last = at;
        while last + 1 < items.len() && items[last + 1].top < next_top - SLACK {
            last += 1;
        }
        pages[index].bottom = match (items.get(last), items.get(last + 1)) {
            (Some(previous), Some(next)) => {
                let block = previous.block(layout);
                let continues = block.is_some() && block == next.block(layout);
                let code = block.is_some_and(|block| {
                    matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. })
                });
                if continues && code {
                    previous.bottom + metrics::CODE_PAD
                } else {
                    next_top
                }
            }
            _ => next_top,
        };
        at = last + 1;
    }
}

/// Assigns every rectangle to the pages it covers, splitting the ones
/// that cross a break. A sweep in top order, carrying the rectangles
/// still open across the boundary, so a code panel spanning twenty pages
/// costs one pass rather than twenty scans.
fn place_rects(layout: &LayoutDoc, pages: &mut [Page]) {
    let mut order: Vec<usize> = (0..layout.rects.len()).collect();
    order.sort_by(|&a, &b| layout.rects[a].y.total_cmp(&layout.rects[b].y));
    let mut next = 0;
    let mut open: Vec<usize> = Vec::new();
    for p in 0..pages.len() {
        let top = pages[p].top;
        let bottom = pages[p].bottom;
        // A rectangle belongs to the page its top falls in, and is drawn
        // as far as that page's content reaches. The two differ only for
        // a panel that carries its padding past the break.
        let next_top = pages.get(p + 1).map_or(f32::INFINITY, |q| q.top);
        while next < order.len() && layout.rects[order[next]].y < next_top {
            open.push(order[next]);
            next += 1;
        }
        let mut pieces: Vec<(usize, DecoRect)> = Vec::new();
        open.retain(|&index| {
            let rect = &layout.rects[index];
            let rect_bottom = rect.y + rect.height;
            let y0 = rect.y.max(top);
            let y1 = rect_bottom.min(bottom);
            if y1 > y0 {
                let mut piece = *rect;
                piece.y = y0;
                piece.height = y1 - y0;
                // A cut edge is square, so the panel reads as continuing.
                if y0 > rect.y {
                    piece.radius_top = 0.0;
                }
                if y1 < rect_bottom {
                    piece.radius_bottom = 0.0;
                }
                pieces.push((index, piece));
            }
            rect_bottom > next_top
        });
        // Back into layout order: paint order is what stacks a table
        // stripe under its grid lines.
        pieces.sort_by_key(|(index, _)| *index);
        pages[p].rects = pieces.into_iter().map(|(_, piece)| piece).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::markdown;
    use crate::doc::model::Document;
    use crate::export::PageSize;
    use crate::layout::{layout, ViewConfig};
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use std::path::PathBuf;

    pub(super) fn laid_out(doc: &Document) -> LayoutDoc {
        laid_out_with_body_size(doc, 11.0)
    }

    pub(super) fn laid_out_with_body_size(doc: &Document, body_size: f32) -> LayoutDoc {
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let cfg = ViewConfig {
            body_size,
            code_size: 9.0,
            zoom: 1.0,
            ..ViewConfig::default()
        };
        layout(
            doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &cfg,
            PageSize::A4.points().0,
        )
    }

    fn many_paragraphs() -> Document {
        markdown::parse(&"A paragraph that says something.\n\n".repeat(120))
    }

    #[test]
    fn no_page_cuts_a_line() {
        let doc = many_paragraphs();
        let l = laid_out(&doc);
        let g = PageGeometry::new(PageSize::A4, 11.0);
        let pages = paginate(&doc, &l, &g);
        assert!(pages.len() > 1, "120 paragraphs need more than one page");
        for page in &pages {
            for run in &l.runs[page.runs.clone()] {
                assert!(run.y >= page.top - 0.01, "nothing sits above the page top");
                let bottom = run.y + metrics::LINE_HEIGHT * run.size;
                assert!(
                    bottom - page.top <= g.content_height() + 0.01,
                    "no line is cut"
                );
            }
        }
    }

    #[test]
    fn every_page_starts_flush_on_its_first_line() {
        let doc = many_paragraphs();
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &PageGeometry::new(PageSize::A4, 11.0));
        for page in &pages {
            assert_eq!(page.top, l.runs[page.runs.start].y, "no stray leading gap");
        }
    }

    #[test]
    fn the_pages_cover_every_run_exactly_once() {
        let doc = many_paragraphs();
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &PageGeometry::new(PageSize::A4, 11.0));
        assert_eq!(pages[0].runs.start, 0);
        for pair in pages.windows(2) {
            assert_eq!(pair[0].runs.end, pair[1].runs.start, "no gap, no overlap");
        }
        assert_eq!(pages.last().unwrap().runs.end, l.runs.len());
    }

    #[test]
    fn an_item_taller_than_a_page_is_placed_anyway() {
        let doc = markdown::parse("# X");
        let l = laid_out_with_body_size(&doc, 400.0);
        let g = PageGeometry::new(PageSize::A4, 11.0);
        let tall = metrics::LINE_HEIGHT * l.runs[0].size;
        assert!(tall > g.content_height(), "the fixture really is oversized");
        let pages = paginate(&doc, &l, &g);
        assert_eq!(pages.len(), 1, "it overflows rather than looping forever");
        assert!(!pages[0].runs.is_empty());
    }

    #[test]
    fn an_empty_document_still_makes_one_page() {
        let doc = markdown::parse("");
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &PageGeometry::new(PageSize::A4, 11.0));
        assert_eq!(pages.len(), 1, "a PDF cannot have zero pages");
        assert!(pages[0].runs.is_empty());
    }

    #[test]
    fn a_panel_crossing_a_break_splits_and_squares_its_cut_corners() {
        let mut fence = String::from("```rust\n");
        for i in 0..90 {
            fence.push_str(&format!("let value_{i} = {i};\n"));
        }
        fence.push_str("```\n");
        let doc = markdown::parse(&fence);
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &PageGeometry::new(PageSize::A4, 11.0));
        assert!(pages.len() > 1, "90 code lines need more than one page");
        let upper = &pages[0].rects[0];
        let lower = &pages[1].rects[0];
        assert!(upper.radius_top > 0.0, "the panel keeps its true top");
        assert_eq!(upper.radius_bottom, 0.0, "square at the cut");
        assert_eq!(lower.radius_top, 0.0, "square at the cut");
        assert!(lower.radius_bottom > 0.0, "and round at its true end");
    }
}

#[cfg(test)]
mod rules {
    use super::tests::{laid_out, laid_out_with_body_size};
    use super::*;
    use crate::doc::images::MediaCache;
    use crate::doc::markdown;
    use crate::doc::model::{BlockKind, Document};
    use crate::export::{PageGeometry, PageSize};
    use crate::layout::{layout, ViewConfig};
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn geometry() -> PageGeometry {
        PageGeometry::new(PageSize::A4, 11.0)
    }

    /// Headings every few paragraphs, each paragraph long enough to wrap,
    /// so page boundaries land in every awkward place there is.
    fn mixed() -> Document {
        let mut src = String::new();
        for section in 0..12 {
            src.push_str(&format!("## Section {section}\n\n"));
            for p in 0..4 {
                src.push_str(&format!(
                    "Paragraph {p} of section {section}, written long enough that it wraps over \
                     several lines of an A4 page and can therefore strand one of them at a \
                     boundary between two pages.\n\n"
                ));
            }
        }
        markdown::parse(&src)
    }

    fn block_of(layout: &LayoutDoc, run: usize) -> usize {
        layout.runs[run].block
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        let doc = mixed();
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        assert!(pages.len() > 3, "the fixture spans several pages");
        for page in &pages[..pages.len() - 1] {
            let last = page.runs.end - 1;
            let kind = &doc.blocks[block_of(&l, last)].kind;
            assert!(
                !matches!(kind, BlockKind::Heading { .. }),
                "a page ends on a heading"
            );
        }
    }

    #[test]
    fn no_block_strands_a_single_line() {
        let doc = mixed();
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        let all = items(&l);
        let mut totals: HashMap<usize, usize> = HashMap::new();
        for item in &all {
            if let Some(block) = item.block(&l) {
                *totals.entry(block).or_default() += 1;
            }
        }
        for page in &pages {
            let mut here: HashMap<usize, usize> = HashMap::new();
            for item in &all {
                let inside = !item.runs.is_empty()
                    && item.runs.start >= page.runs.start
                    && item.runs.end <= page.runs.end;
                if inside {
                    if let Some(block) = item.block(&l) {
                        *here.entry(block).or_default() += 1;
                    }
                }
            }
            for (block, count) in here {
                let total = totals[&block];
                assert!(
                    count >= 2 || count == total,
                    "block {block} left {count} of its {total} lines alone"
                );
            }
        }
    }

    #[test]
    fn a_table_row_never_splits() {
        let mut src = String::from("Filler paragraph.\n\n".repeat(30).as_str());
        src.push_str("| left | right |\n|---|---|\n");
        for row in 0..40 {
            src.push_str(&format!("| cell {row} | value {row} |\n"));
        }
        let doc = markdown::parse(&src);
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        assert!(!l.table_rows.is_empty(), "the fixture has a table");
        for page in &pages {
            for row in &l.table_rows {
                assert!(
                    page.top <= row.top + SLACK || page.top >= row.bottom - SLACK,
                    "a page starts inside a row band"
                );
            }
        }
    }

    #[test]
    fn an_image_is_never_cut_by_a_break() {
        let mut src = String::from("Filler paragraph.\n\n".repeat(28).as_str());
        src.push_str("![logo](oryx-test.png)\n\n");
        src.push_str(&"Filler paragraph.\n\n".repeat(28));
        let doc = markdown::parse(&src);
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        assert!(!l.images.is_empty(), "the fixture has an image");
        for page in &pages {
            for image in &l.images {
                assert!(
                    page.top <= image.y + SLACK || page.top >= image.y + image.height - SLACK,
                    "a page starts inside an image"
                );
            }
        }
    }

    /// The defect a screenshot showed in the app: an image near the foot
    /// of a page ran off the sheet, because the fit was measured over
    /// text lines and an image is not one. The sweep brackets a page
    /// boundary so the image lands at the bottom in some of its runs.
    #[test]
    fn an_image_never_runs_past_the_bottom_of_its_page() {
        let g = geometry();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let cfg = ViewConfig {
            body_size: 11.0,
            code_size: 9.0,
            zoom: 1.0,
            ..ViewConfig::default()
        };
        let mut after_text = false;
        for filler in 14..40 {
            let mut src = "Filler paragraph here.\n\n".repeat(filler);
            src.push_str("![logo](oryx-test.png)\n\n");
            src.push_str(&"Filler paragraph here.\n\n".repeat(6));
            let doc = markdown::parse(&src);
            let l = layout(
                &doc,
                &Theme::default_dark(),
                &mut fonts,
                &mut media,
                &cfg,
                PageSize::A4.points().0,
            );
            let pages = paginate(&doc, &l, &g);
            for (index, page) in pages.iter().enumerate() {
                for image in &page.images {
                    assert!(
                        image.y + image.height - page.top <= g.content_height() + SLACK,
                        "an image runs past the foot of page {index} with {filler} fillers"
                    );
                    // The telling case is the image sitting below text
                    // rather than opening a page of its own.
                    after_text |= image.y > page.top + SLACK;
                }
            }
        }
        assert!(after_text, "the sweep put an image below text on a page");
    }

    #[test]
    fn an_image_taller_than_a_page_is_scaled_to_fit() {
        let doc = markdown::parse("![logo](oryx-test.png)\n");
        // A tiny page makes any image oversized without a fixture file.
        let g = PageGeometry::new(PageSize::A4, 200.0);
        let l = laid_out_with_body_size(&doc, 11.0);
        let source = l.images[0].clone();
        assert!(
            source.height > g.content_height(),
            "the fixture is oversized"
        );
        let pages = paginate(&doc, &l, &g);
        let placed = pages.iter().flat_map(|p| p.images.iter()).next().unwrap();
        assert!(placed.height <= g.content_height() + SLACK, "scaled to fit");
        let before = source.width / source.height;
        let after = placed.width / placed.height;
        assert!((before - after).abs() < 0.01, "aspect kept");
    }

    /// The defect a split panel showed in the app: the page's clipping
    /// edge sat one padding above its last line, so the panel stopped in
    /// the middle of the text it was meant to hold.
    #[test]
    fn a_split_panel_covers_every_line_it_carries() {
        let mut fence = String::from("```rust\n");
        for i in 0..120 {
            fence.push_str(&format!("let value_{i} = {i};\n"));
        }
        fence.push_str("```\n");
        let doc = markdown::parse(&fence);
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        assert!(pages.len() > 1, "the fence spans pages");
        for page in &pages {
            let panel = page
                .rects
                .iter()
                .find(|rect| rect.stroke == 0.0)
                .expect("the panel is on every page the block reaches");
            for run in &l.runs[page.runs.clone()] {
                let bottom = run.y + metrics::LINE_HEIGHT * run.size;
                assert!(
                    run.y >= panel.y - SLACK,
                    "a line sits above the panel that holds it"
                );
                assert!(
                    bottom <= panel.y + panel.height + SLACK,
                    "a line falls past the bottom of the panel that holds it"
                );
            }
        }
    }

    /// The defect a badge row showed in the app: a document that opens
    /// on images has no run above them, so trimming the page to its first
    /// line pushed them off the top of the sheet.
    #[test]
    fn a_document_opening_on_an_image_keeps_it_on_the_page() {
        let doc = markdown::parse("![logo](oryx-test.png)\n\nText below the image.\n");
        let l = laid_out(&doc);
        assert!(!l.images.is_empty(), "the fixture has an image");
        let pages = paginate(&doc, &l, &geometry());
        let image = &l.images[0];
        assert!(image.y < l.runs[0].y, "the image is above the first line");
        assert!(
            (pages[0].top - image.y).abs() < SLACK,
            "the page starts at the image, not at the first line below it"
        );
    }

    #[test]
    fn a_continued_code_panel_keeps_its_padding() {
        let mut fence = String::from("```rust\n");
        for i in 0..120 {
            fence.push_str(&format!("let value_{i} = {i};\n"));
        }
        fence.push_str("```\n");
        let doc = markdown::parse(&fence);
        let l = laid_out(&doc);
        let pages = paginate(&doc, &l, &geometry());
        assert!(pages.len() > 1, "the fence spans pages");
        for page in &pages[1..] {
            let first = l.runs[page.runs.start].y;
            assert!(
                (first - page.top - metrics::CODE_PAD).abs() < SLACK,
                "a continued panel starts one padding above its first line"
            );
        }
    }
}

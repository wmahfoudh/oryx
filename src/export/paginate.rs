//! Cutting one tall column into pages. Pure: layout and page geometry in,
//! a list of pages out, with no fonts and no PDF anywhere near it.

use std::ops::Range;

use std::collections::HashMap;

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
    pub runs: Range<usize>,
    pub images: Vec<ImagePlace>,
    pub rects: Vec<DecoRect>,
}

/// One visual line and the box it occupies. Runs that overlap vertically
/// belong to the same line, which keeps a raised footnote reference with
/// the words it follows and a table row's cells together.
struct Line {
    runs: Range<usize>,
    top: f32,
    bottom: f32,
}

fn lines(layout: &LayoutDoc) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, run) in layout.runs.iter().enumerate() {
        let bottom = run.y + metrics::LINE_HEIGHT * run.size;
        match out.last_mut() {
            Some(line) if run.y < line.bottom - SLACK => {
                line.runs.end = i + 1;
                line.top = line.top.min(run.y);
                line.bottom = line.bottom.max(bottom);
            }
            _ => out.push(Line {
                runs: i..i + 1,
                top: run.y,
                bottom,
            }),
        }
    }
    out
}

/// Where each block's lines begin and end, so the widow rule costs a
/// lookup rather than a scan.
fn block_spans(layout: &LayoutDoc, lines: &[Line]) -> HashMap<usize, (usize, usize)> {
    let mut spans: HashMap<usize, (usize, usize)> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let block = layout.runs[line.runs.start].block;
        spans
            .entry(block)
            .and_modify(|span| span.1 = index)
            .or_insert((index, index));
    }
    spans
}

/// Whether a break before line `k` is one of the ones that read badly.
/// The page currently starts at line `i`.
fn rejected(
    doc: &Document,
    layout: &LayoutDoc,
    lines: &[Line],
    spans: &HashMap<usize, (usize, usize)>,
    i: usize,
    k: usize,
) -> bool {
    let previous = layout.runs[lines[k - 1].runs.start].block;
    let next = layout.runs[lines[k].runs.start].block;
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
    // An image moves whole rather than being cut in half. A table row
    // needs no rule here: its cells shape into one line group, and the
    // page top is pulled back to the band so the padding travels with it.
    let y = lines[k].top;
    layout
        .images
        .iter()
        .any(|image| y > image.y + SLACK && y < image.y + image.height - SLACK)
}

/// Where a page starts, given the first line it carries. Decoration that
/// sits above that line and belongs to it comes too: a continued code
/// panel keeps its inner padding, and a table row starts at its band so
/// the stripe is not left behind on the page before.
fn page_top(doc: &Document, layout: &LayoutDoc, lines: &[Line], i: usize) -> f32 {
    let line = &lines[i];
    let block = layout.runs[line.runs.start].block;
    let continued = i > 0 && layout.runs[lines[i - 1].runs.start].block == block;
    if continued && matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. }) {
        return line.top - metrics::CODE_PAD;
    }
    let row = layout
        .table_rows
        .partition_point(|band| band.top <= line.top + SLACK)
        .saturating_sub(1);
    if let Some(band) = layout.table_rows.get(row) {
        if line.top > band.top && line.top < band.bottom {
            return band.top;
        }
    }
    line.top
}

/// Cuts the column into pages. A page holds as many whole lines as its
/// content box takes, and starts flush on the first of them, so the
/// layout's own vertical margin never doubles with the page's.
pub fn paginate(doc: &Document, layout: &LayoutDoc, geometry: &PageGeometry) -> Vec<Page> {
    let content = geometry.content_height();
    let lines = lines(layout);
    let spans = block_spans(layout, &lines);
    let mut pages: Vec<Page> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let top = page_top(doc, layout, &lines, i);
        let mut j = i;
        while j < lines.len() && lines[j].bottom - top <= content + SLACK {
            j += 1;
        }
        // A line taller than the whole content box is placed anyway. It
        // overflows, and the pass moves on instead of stalling here.
        if j == i {
            j = i + 1;
        }
        if j < lines.len() {
            let mut candidate = j;
            loop {
                if !rejected(doc, layout, &lines, &spans, i, candidate) {
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
        pages.push(Page {
            top,
            runs: lines[i].runs.start..lines[j - 1].runs.end,
            images: Vec::new(),
            rects: Vec::new(),
        });
        i = j;
    }
    if pages.is_empty() {
        pages.push(Page {
            top: 0.0,
            runs: 0..0,
            images: Vec::new(),
            rects: Vec::new(),
        });
    }
    open_first_page(layout, &mut pages[0]);
    place_rects(layout, &mut pages);
    place_images(layout, &mut pages, geometry.content_height());
    pages
}

/// Lowers the first page's top to any decoration that reaches across it.
/// A document opening on a code fence starts with the panel's padding
/// above its first line, and that padding belongs to the page rather than
/// being clipped off the top of it. Later pages take their top from the
/// break, where a crossing rectangle is split square instead.
fn open_first_page(layout: &LayoutDoc, first: &mut Page) {
    first.top = layout
        .rects
        .iter()
        .filter(|rect| rect.y < first.top && rect.y + rect.height > first.top)
        .map(|rect| rect.y)
        .fold(first.top, f32::min);
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
        let split = pages.get(p + 1).map_or(f32::INFINITY, |q| q.top);
        while next < order.len() && layout.rects[order[next]].y < split {
            open.push(order[next]);
            next += 1;
        }
        let mut pieces: Vec<(usize, DecoRect)> = Vec::new();
        open.retain(|&index| {
            let rect = &layout.rects[index];
            let rect_bottom = rect.y + rect.height;
            let y0 = rect.y.max(top);
            let y1 = rect_bottom.min(split);
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
            rect_bottom > split
        });
        // Back into layout order: paint order is what stacks a table
        // stripe under its grid lines.
        pieces.sort_by_key(|(index, _)| *index);
        pages[p].rects = pieces.into_iter().map(|(_, piece)| piece).collect();
    }
}

/// Puts each image on the page its top falls in. A break never cuts one,
/// so the page it starts on is the page that holds it.
fn place_images(layout: &LayoutDoc, pages: &mut [Page], content: f32) {
    for image in &layout.images {
        let page = pages
            .partition_point(|page| page.top <= image.y)
            .saturating_sub(1);
        let mut placed = image.clone();
        // One taller than a whole page cannot be moved anywhere useful,
        // so it is scaled to the content box and keeps its aspect.
        if placed.height > content {
            let scale = content / placed.height;
            placed.width *= scale;
            placed.height *= scale;
        }
        pages[page].images.push(placed);
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
    use crate::doc::markdown;
    use crate::doc::model::{BlockKind, Document};
    use crate::export::{PageGeometry, PageSize};
    use std::collections::HashMap;

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
        let all = lines(&l);
        let mut totals: HashMap<usize, usize> = HashMap::new();
        for line in &all {
            *totals.entry(block_of(&l, line.runs.start)).or_default() += 1;
        }
        for page in &pages {
            let mut here: HashMap<usize, usize> = HashMap::new();
            for line in &all {
                if line.runs.start >= page.runs.start && line.runs.end <= page.runs.end {
                    *here.entry(block_of(&l, line.runs.start)).or_default() += 1;
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

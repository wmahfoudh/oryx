//! Cutting one tall column into pages. Pure: layout and page geometry in,
//! a list of pages out, with no fonts and no PDF anywhere near it.

use std::collections::HashMap;
use std::ops::Range;

use crate::doc::model::{BlockKind, Document};
use crate::export::PageGeometry;
use crate::layout::{metrics, DecoRect, ImagePlace, LayoutDoc, MathGlyph};

/// Float slack for comparing positions that arithmetic should have made
/// equal.
const SLACK: f32 = 0.01;

/// One page: where it starts in document coordinates, the runs, images
/// and math glyphs it carries, and its rectangles already split at the
/// break.
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
    pub math: Vec<MathGlyph>,
}

/// One thing a page can hold whole: a visual line, an image, an equation,
/// or a line with an image or equation sitting in it. Pieces that overlap
/// vertically belong to the same item, which keeps a raised footnote
/// reference with the words it follows, an inline badge or equation with
/// its sentence, and a table row's cells together. An item is atomic, so
/// nothing here is ever cut in half.
struct Item {
    runs: Range<usize>,
    images: Vec<usize>,
    math: Range<usize>,
    top: f32,
    bottom: f32,
}

impl Item {
    /// The block an item belongs to, absent for an image standing alone.
    fn block(&self, layout: &LayoutDoc) -> Option<usize> {
        if !self.runs.is_empty() {
            return Some(layout.runs[self.runs.start].block);
        }
        (!self.math.is_empty()).then(|| layout.math_glyphs[self.math.start].block)
    }
}

/// Every run, image and math glyph, in document order, grouped into items.
fn items(layout: &LayoutDoc) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    group_into(layout, &mut out, &mut 0, &mut 0, &mut 0);
    out
}

/// The grouping core, resumable: continues from where the cursors
/// stopped, growing the last item where new pieces overlap it. On equal
/// tops a run wins over an image, an image over a math glyph, so the
/// order is deterministic whatever the growth cadence.
fn group_into(
    layout: &LayoutDoc,
    out: &mut Vec<Item>,
    run_seen: &mut usize,
    image_seen: &mut usize,
    math_seen: &mut usize,
) {
    #[derive(Clone, Copy, PartialEq)]
    enum Take {
        Run,
        Image,
        Math,
    }
    let (mut run, mut image, mut math) = (*run_seen, *image_seen, *math_seen);
    loop {
        let mut take: Option<(f32, Take)> = None;
        if let Some(r) = layout.runs.get(run) {
            take = Some((r.y, Take::Run));
        }
        if let Some(i) = layout.images.get(image) {
            if !take.is_some_and(|(top, _)| top <= i.y) {
                take = Some((i.y, Take::Image));
            }
        }
        if let Some(g) = layout.math_glyphs.get(math) {
            if !take.is_some_and(|(top, _)| top <= g.top) {
                take = Some((g.top, Take::Math));
            }
        }
        let Some((_, take)) = take else {
            break;
        };
        match take {
            Take::Run => {
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
                        math: math..math,
                        top: piece.y,
                        bottom,
                    }),
                }
                run += 1;
            }
            Take::Image => {
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
                        math: math..math,
                        top: piece.y,
                        bottom,
                    }),
                }
                image += 1;
            }
            Take::Math => {
                let piece = &layout.math_glyphs[math];
                match out.last_mut() {
                    Some(item) if piece.top < item.bottom - SLACK => {
                        item.math.end = math + 1;
                        item.top = item.top.min(piece.top);
                        item.bottom = item.bottom.max(piece.bottom);
                    }
                    _ => out.push(Item {
                        runs: run..run,
                        images: Vec::new(),
                        math: math..math + 1,
                        top: piece.top,
                        bottom: piece.bottom,
                    }),
                }
                math += 1;
            }
        }
    }
    *run_seen = run;
    *image_seen = image;
    *math_seen = math;
}

/// Grows each item to cover the decoration that belongs to it and hangs
/// below its text: a table row's stripe carries padding under its cells,
/// and a code block's panel closes one padding below its last line. The
/// fit is measured against these, so nothing reaches into the margin the
/// page number sits in. A row band taller than the content box is left
/// alone: it can never travel whole, so its lines flow like a
/// paragraph's instead of dripping one to a page.
fn extend_items(doc: &Document, layout: &LayoutDoc, content: f32, items: &mut [Item]) {
    for item in items.iter_mut() {
        item.bottom = extended_bottom(doc, layout, content, item);
    }
}

/// One item's extended bottom over its raw shaped one, the value the
/// fit is measured against.
fn extended_bottom(doc: &Document, layout: &LayoutDoc, content: f32, item: &Item) -> f32 {
    let block = item.block(layout);
    let mut bottom = item.bottom;
    let row = layout
        .table_rows
        .partition_point(|band| band.top <= item.top + SLACK)
        .saturating_sub(1);
    if let Some(band) = layout.table_rows.get(row) {
        let fits = band.bottom - band.top <= content + SLACK;
        if fits && item.top >= band.top - SLACK && item.top < band.bottom {
            bottom = bottom.max(band.bottom);
        }
    }
    // Every code line reserves the panel's closing padding, not just
    // the block's last: a panel that carries on overleaf closes on
    // this page too, one padding below whichever line ends it.
    let code =
        block.is_some_and(|block| matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. }));
    if code {
        bottom += metrics::CODE_PAD;
    }
    // A run with an Image block is a placeholder's alt text; the box
    // closes one padding below it and the fit carries the border too.
    let placeholder =
        block.is_some_and(|block| matches!(doc.blocks[block].kind, BlockKind::Image { .. }));
    if placeholder {
        bottom += metrics::PLACEHOLDER_PAD;
    }
    bottom
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
/// the stripe is not left behind on the page before. `first` marks the
/// document's own first page, whose top backs up over everything drawn
/// above its first item.
fn page_top(
    doc: &Document,
    layout: &LayoutDoc,
    items: &[Item],
    content: f32,
    i: usize,
    first: bool,
) -> f32 {
    let item = &items[i];
    if let Some(block) = item.block(layout) {
        // A code line starts its page one padding up whether the panel
        // continues or opens fresh here: the rect begins there either
        // way, and a top left past the boundary strands an empty strip
        // of it on the page before.
        if matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. }) {
            return item.top - metrics::CODE_PAD;
        }
        // A placeholder's alt text brings its whole box: the border
        // opens one padding above the text.
        if matches!(doc.blocks[block].kind, BlockKind::Image { .. }) {
            return item.top - metrics::PLACEHOLDER_PAD;
        }
    }
    let row = layout
        .table_rows
        .partition_point(|band| band.top <= item.top + SLACK)
        .saturating_sub(1);
    let mut top = item.top;
    if let Some(band) = layout.table_rows.get(row) {
        // An over-tall band flows across pages, so a page opening inside
        // it starts at its own first line, not back at the band.
        let fits = band.bottom - band.top <= content + SLACK;
        if fits && item.top > band.top && item.top < band.bottom {
            top = band.top;
        }
    }
    if first {
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
    let mut items = items(layout);
    extend_items(doc, layout, content, &mut items);
    let spans = block_spans(layout, &items);
    let mut pages: Vec<Page> = Vec::new();
    let mut i = 0;
    let mut cursor = 0;
    let mut math_cursor = 0;
    while i < items.len() {
        let top = page_top(doc, layout, &items, content, i, i == 0);
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
        let math_end = items[j - 1].math.end.max(math_cursor);
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
            math: layout.math_glyphs[math_cursor..math_end].to_vec(),
        });
        cursor = end;
        math_cursor = math_end;
        i = j;
    }
    if pages.is_empty() {
        pages.push(Page {
            top: 0.0,
            bottom: f32::INFINITY,
            runs: 0..0,
            images: Vec::new(),
            rects: Vec::new(),
            math: Vec::new(),
        });
    }
    close_pages(&items, &mut pages);
    place_rects(layout, &mut pages);
    pages
}

/// Cuts pages incrementally while the layout streams. `advance` reads
/// the grown layout, closes every page whose break is already decidable
/// behind the cursor, and answers the pages that became final, in
/// order; `complete` closes the tail. The pages equal the one-shot
/// `paginate`'s, whatever the growth cadence: the same grouping, fit,
/// break and rectangle rules run over the same items, only paced by
/// what is placed.
#[derive(Default)]
pub struct Paginator {
    /// Items grouped so far; entries before `settled` carry extended
    /// bottoms and final grouping, the tail past it may still grow.
    items: Vec<Item>,
    runs_seen: usize,
    images_seen: usize,
    math_seen: usize,
    settled: usize,
    spans: HashMap<usize, (usize, usize)>,
    /// First item of the open page and the run and math cursors behind it.
    next_item: usize,
    cursor: usize,
    math_cursor: usize,
    /// Where the close walk for page bottoms has reached.
    close_at: usize,
    /// The rectangle sweep: unswept indices sorted by (y, index), and
    /// the ones still open across the last page boundary.
    pending_rects: Vec<usize>,
    rects_seen: usize,
    open_rects: Vec<usize>,
    /// Images carried onto emitted pages, ready to drop.
    images_done: usize,
    /// Whether the document's first page has closed, whose top alone
    /// backs up over everything drawn above its first item.
    started: bool,
    emitted: usize,
}

/// What emitted pages no longer need: the front of each element vector
/// a fused export may drop once it has drained the paginator.
pub struct Consumed {
    pub runs: usize,
    pub rects: usize,
    pub images: usize,
    pub math: usize,
}

impl Paginator {
    pub fn new() -> Paginator {
        Paginator::default()
    }

    pub fn advance(
        &mut self,
        doc: &Document,
        layout: &LayoutDoc,
        geometry: &PageGeometry,
        complete: bool,
    ) -> Vec<Page> {
        let content = geometry.content_height();
        group_into(
            layout,
            &mut self.items,
            &mut self.runs_seen,
            &mut self.images_seen,
            &mut self.math_seen,
        );
        self.settle(doc, layout, content, complete);
        let mut out = Vec::new();
        while let Some(page) = self.try_close(doc, layout, content, complete) {
            out.push(page);
        }
        if complete && !self.started && self.emitted == 0 && out.is_empty() {
            let rects = self.assign_rects(layout, 0.0, f32::INFINITY, f32::INFINITY);
            out.push(Page {
                top: 0.0,
                bottom: f32::INFINITY,
                runs: 0..0,
                images: Vec::new(),
                rects,
                math: Vec::new(),
            });
        }
        self.emitted += out.len();
        out
    }

    /// Finalizes items the grouping can no longer touch: extended
    /// bottoms and the block spans the break rules read.
    fn settle(&mut self, doc: &Document, layout: &LayoutDoc, content: f32, complete: bool) {
        let done = if complete {
            self.items.len()
        } else {
            self.items.len().saturating_sub(1)
        };
        for index in self.settled..done {
            let bottom = extended_bottom(doc, layout, content, &self.items[index]);
            self.items[index].bottom = bottom;
            if let Some(block) = self.items[index].block(layout) {
                self.spans
                    .entry(block)
                    .and_modify(|span| span.1 = index)
                    .or_insert((index, index));
            }
        }
        self.settled = done.max(self.settled);
    }

    /// Whether a page ending before item `j` can be emitted now: its
    /// break rules and its rectangles need the blocks it touches to be
    /// behind the pass, or, inside an unquoted code block, one more
    /// line of lookahead past the break.
    fn closable(&self, doc: &Document, layout: &LayoutDoc, j: usize, complete: bool) -> bool {
        if complete {
            return true;
        }
        let Some(block) = self.items[j].block(layout) else {
            return true;
        };
        let behind = layout
            .block_position(block)
            .is_some_and(|position| position + 1 < layout.placed_positions());
        if behind {
            return true;
        }
        let open_code = doc.blocks[block].quote_depth == 0
            && matches!(doc.blocks[block].kind, BlockKind::CodeBlock { .. });
        open_code && self.spans.get(&block).is_some_and(|(_, last)| *last > j)
    }

    /// Closes one page if its break is decidable, mirroring the
    /// one-shot loop body exactly.
    fn try_close(
        &mut self,
        doc: &Document,
        layout: &LayoutDoc,
        content: f32,
        complete: bool,
    ) -> Option<Page> {
        let i = self.next_item;
        if i >= self.items.len() {
            return None;
        }
        if !complete && i >= self.settled {
            return None;
        }
        let top = page_top(doc, layout, &self.items, content, i, !self.started);
        let mut j = i;
        while j < self.settled && self.items[j].bottom - top <= content + SLACK {
            j += 1;
        }
        if j >= self.settled && !complete {
            // Everything settled still fits; more may join the page.
            return None;
        }
        // An item taller than the whole content box is placed anyway.
        if j == i {
            j = i + 1;
        }
        if j < self.items.len() {
            if !self.closable(doc, layout, j, complete) {
                return None;
            }
            let mut candidate = j;
            loop {
                if !rejected(doc, layout, &self.items, &self.spans, i, candidate) {
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
        let end = self.items[j - 1].runs.end.max(self.cursor);
        let images: Vec<ImagePlace> = self.items[i..j]
            .iter()
            .flat_map(|item| item.images.iter())
            .map(|index| scaled(&layout.images[*index], content))
            .collect();
        for item in &self.items[i..j] {
            for &image in &item.images {
                self.images_done = self.images_done.max(image + 1);
            }
        }
        let (bottom, next_top) = if j < self.items.len() {
            let next_top = page_top(doc, layout, &self.items, content, j, false);
            let mut last = self.close_at;
            while last + 1 < self.items.len() && self.items[last + 1].top < next_top - SLACK {
                last += 1;
            }
            self.close_at = last + 1;
            (next_top.max(self.items[last].bottom), next_top)
        } else {
            self.close_at = self.items.len();
            (f32::INFINITY, f32::INFINITY)
        };
        let rects = self.assign_rects(layout, top, bottom, next_top);
        let math_end = self.items[j - 1].math.end.max(self.math_cursor);
        let math = layout.math_glyphs[self.math_cursor..math_end].to_vec();
        self.math_cursor = math_end;
        self.started = true;
        let start = self.cursor;
        self.cursor = end;
        self.next_item = j;
        Some(Page {
            top,
            bottom,
            runs: start..end,
            images,
            rects,
            math,
        })
    }

    /// Hands over what the emitted pages no longer need and rebases the
    /// paginator's indices onto the drained layout: the caller drops
    /// exactly the answered counts from the front of the element
    /// vectors before the next advance.
    pub fn consume(&mut self) -> Consumed {
        let runs = self.cursor;
        let images = self.images_done;
        let math = self.math_cursor;
        let rects = self
            .open_rects
            .iter()
            .chain(self.pending_rects.iter())
            .copied()
            .min()
            .unwrap_or(self.rects_seen);
        let items_done = self.next_item.min(self.close_at);
        self.items.drain(..items_done);
        for item in &mut self.items {
            item.runs = item.runs.start - runs..item.runs.end - runs;
            item.math = item.math.start - math..item.math.end - math;
            for image in &mut item.images {
                *image -= images;
            }
        }
        self.next_item -= items_done;
        self.close_at -= items_done;
        self.settled -= items_done;
        for span in self.spans.values_mut() {
            span.0 = span.0.saturating_sub(items_done);
            span.1 = span.1.saturating_sub(items_done);
        }
        self.cursor = 0;
        self.math_cursor = 0;
        self.runs_seen -= runs;
        self.images_seen -= images;
        self.images_done = 0;
        self.math_seen -= math;
        self.rects_seen -= rects;
        for index in self.open_rects.iter_mut().chain(&mut self.pending_rects) {
            *index -= rects;
        }
        Consumed {
            runs,
            rects,
            images,
            math,
        }
    }

    /// The rectangle sweep for one closed page, the one-shot
    /// `place_rects` paced by page: rectangles opening before the next
    /// page's top join, pieces cut to this page's extent, and whatever
    /// reaches past the boundary stays open for the next one.
    fn assign_rects(
        &mut self,
        layout: &LayoutDoc,
        top: f32,
        bottom: f32,
        next_top: f32,
    ) -> Vec<DecoRect> {
        for index in self.rects_seen..layout.rects.len() {
            self.pending_rects.push(index);
        }
        self.rects_seen = layout.rects.len();
        self.pending_rects.sort_by(|&a, &b| {
            layout.rects[a]
                .y
                .total_cmp(&layout.rects[b].y)
                .then(a.cmp(&b))
        });
        let opening = self
            .pending_rects
            .partition_point(|&index| layout.rects[index].y < next_top);
        self.open_rects.extend(self.pending_rects.drain(..opening));
        let mut pieces: Vec<(usize, DecoRect)> = Vec::new();
        self.open_rects.retain(|&index| {
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
        pieces.into_iter().map(|(_, piece)| piece).collect()
    }
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
/// next begins, and past that only where an item reaches further, which
/// is how a code panel keeps its padding below the last line it carries.
fn close_pages(items: &[Item], pages: &mut [Page]) {
    let mut at = 0;
    for index in 0..pages.len().saturating_sub(1) {
        let next_top = pages[index + 1].top;
        // The last item of this page is the one before the first item of
        // the next, which starts at the next page's own top.
        let mut last = at;
        while last + 1 < items.len() && items[last + 1].top < next_top - SLACK {
            last += 1;
        }
        // An item already covers the decoration that hangs below it, so
        // its own bottom is where a code panel closes; everything else
        // ends where the next page begins.
        pages[index].bottom = match items.get(last) {
            Some(previous) => next_top.max(previous.bottom),
            None => next_top,
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
    use crate::export::{Orientation, PageSize};
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
        markdown::parse("A paragraph that says something.\n\n".repeat(120).as_str())
    }

    #[test]
    fn no_page_cuts_a_line() {
        let doc = many_paragraphs();
        let l = laid_out(&doc);
        let g = PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0);
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
        let pages = paginate(
            &doc,
            &l,
            &PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0),
        );
        for page in &pages {
            assert_eq!(page.top, l.runs[page.runs.start].y, "no stray leading gap");
        }
    }

    #[test]
    fn the_pages_cover_every_run_exactly_once() {
        let doc = many_paragraphs();
        let l = laid_out(&doc);
        let pages = paginate(
            &doc,
            &l,
            &PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0),
        );
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
        let g = PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0);
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
        let pages = paginate(
            &doc,
            &l,
            &PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0),
        );
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
        let doc = markdown::parse(fence.as_str());
        let l = laid_out(&doc);
        let pages = paginate(
            &doc,
            &l,
            &PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0),
        );
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
    use crate::export::{Orientation, PageGeometry, PageSize};
    use crate::layout::{layout, ViewConfig};
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn geometry() -> PageGeometry {
        PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0)
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
        markdown::parse(src.as_str())
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
        let doc = markdown::parse(src.as_str());
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
        let doc = markdown::parse(src.as_str());
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
            let doc = markdown::parse(src.as_str());
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

    /// The defect a page number showed in the app: a table row's stripe
    /// carries padding below its cells and a code panel closes one
    /// padding below its last line, so either could put a border into the
    /// margin the number sits in. The sweep slides the boundary through
    /// the table and the fence a line at a time.
    #[test]
    fn nothing_drawn_reaches_into_the_bottom_margin() {
        let g = geometry();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
        let cfg = ViewConfig {
            body_size: 11.0,
            code_size: 9.0,
            zoom: 1.0,
            ..ViewConfig::default()
        };
        for filler in 0..30 {
            let mut src = "Filler paragraph here.\n\n".repeat(filler);
            src.push_str("| Shortcut | Action |\n|---|---|\n");
            for row in 0..12 {
                src.push_str(&format!("| Ctrl+{row} | Does thing {row} |\n"));
            }
            src.push_str("\n```rust\n");
            for line in 0..12 {
                src.push_str(&format!("let value_{line} = {line};\n"));
            }
            src.push_str("```\n");
            let doc = markdown::parse(src.as_str());
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
                for rect in &page.rects {
                    assert!(
                        rect.y + rect.height - page.top <= g.content_height() + SLACK,
                        "page {index} draws into the bottom margin with {filler} fillers"
                    );
                }
            }
        }
    }

    #[test]
    fn an_image_taller_than_a_page_is_scaled_to_fit() {
        let doc = markdown::parse("![logo](oryx-test.png)\n");
        // A tiny page makes any image oversized without a fixture file.
        let g = PageGeometry::new(PageSize::A4, Orientation::Portrait, 200.0);
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
        let doc = markdown::parse(fence.as_str());
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
        let doc = markdown::parse(fence.as_str());
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

    /// The defect an exported README showed: a missing image's
    /// placeholder box split at a page break, an empty strip of it
    /// overlapping the page number while the alt text opened the next
    /// page. The sweep slides the box toward the boundary a paragraph
    /// at a time.
    #[test]
    fn a_placeholder_box_paginates_as_one_atom() {
        let g = geometry();
        let mut split_seen = false;
        for filler in 8..40 {
            let mut src = "Filler paragraph here.\n\n".repeat(filler);
            src.push_str("![Oryx rendering tables](missing/tables.png)\n\n");
            src.push_str(&"Filler paragraph here.\n\n".repeat(6));
            let doc = markdown::parse(src.as_str());
            let l = laid_out(&doc);
            assert!(!l.rects.is_empty(), "the missing image draws a placeholder");
            let pages = paginate(&doc, &l, &g);
            split_seen |= pages.len() > 1;
            for page in &pages {
                for rect in &l.rects {
                    assert!(
                        page.top <= rect.y + SLACK || page.top >= rect.y + rect.height - SLACK,
                        "a page starts inside the placeholder with {filler} fillers"
                    );
                }
                for rect in &page.rects {
                    assert!(
                        rect.y + rect.height - page.top <= g.content_height() + SLACK,
                        "the placeholder reaches into the bottom margin with {filler} fillers"
                    );
                }
            }
        }
        assert!(split_seen, "the sweep drove the box across a boundary");
    }

    /// The defect an exported page showed: a code panel that opens a
    /// fresh page left an empty strip of itself on the page before,
    /// carrying none of its lines.
    #[test]
    fn every_panel_fragment_carries_a_line() {
        let g = geometry();
        for filler in 0..30 {
            let mut src = "Filler paragraph here.\n\n".repeat(filler);
            src.push_str("```rust\n");
            for line in 0..12 {
                src.push_str(&format!("let value_{line} = {line};\n"));
            }
            src.push_str("```\n");
            let doc = markdown::parse(src.as_str());
            let l = laid_out(&doc);
            let pages = paginate(&doc, &l, &g);
            for (index, page) in pages.iter().enumerate() {
                for rect in &page.rects {
                    let holds_a_line = l.runs[page.runs.clone()].iter().any(|run| {
                        let bottom = run.y + metrics::LINE_HEIGHT * run.size;
                        run.y < rect.y + rect.height + SLACK && bottom > rect.y - SLACK
                    });
                    assert!(
                        holds_a_line,
                        "page {index} draws an empty panel fragment with {filler} fillers"
                    );
                }
            }
        }
    }

    /// Rows whose cells wrap over several lines stay whole wherever the
    /// boundary falls; only a row taller than the page itself may split.
    #[test]
    fn a_row_that_fits_a_page_never_splits_wherever_the_break_falls() {
        let g = geometry();
        for filler in 0..30 {
            let mut src = "Filler paragraph here.\n\n".repeat(filler);
            src.push_str("| Setting | What it does |\n|---|---|\n");
            for row in 0..8 {
                src.push_str(&format!(
                    "| option-{row} | A cell written long enough that it wraps over \
                     several lines inside its column and makes the row band tall {row}. |\n"
                ));
            }
            let doc = markdown::parse(src.as_str());
            let l = laid_out(&doc);
            let pages = paginate(&doc, &l, &g);
            for page in &pages {
                for row in &l.table_rows {
                    if row.bottom - row.top > g.content_height() {
                        continue;
                    }
                    assert!(
                        page.top <= row.top + SLACK || page.top >= row.bottom - SLACK,
                        "a page starts inside a fitting row with {filler} fillers"
                    );
                }
            }
        }
    }

    /// A row taller than the page cannot stay atomic, so it flows like
    /// a paragraph: pages fill with its lines, the grid clips at the
    /// content box with a closing edge, and the number's margin stays
    /// clear. The field showed the alternative: one line per page for a
    /// hundred pages, columns drawn through the page number.
    #[test]
    fn a_row_taller_than_a_page_flows_and_loses_nothing() {
        let g = geometry();
        let cell = "A very long cell indeed. ".repeat(400);
        let src = format!("| Setting | What it does |\n|---|---|\n| tall | {cell} |\n");
        let doc = markdown::parse(src.as_str());
        let l = laid_out(&doc);
        let band = &l.table_rows[l.table_rows.len() - 1];
        assert!(
            band.bottom - band.top > g.content_height(),
            "the fixture row really is taller than a page"
        );
        let pages = paginate(&doc, &l, &g);
        assert!(pages.len() > 1, "the row spans pages");
        assert_eq!(pages[0].runs.start, 0);
        for pair in pages.windows(2) {
            assert_eq!(pair[0].runs.end, pair[1].runs.start, "no gap, no overlap");
        }
        assert_eq!(pages.last().unwrap().runs.end, l.runs.len());
        let lines = l.runs.len();
        assert!(
            pages.len() < lines / 8,
            "{lines} lines drip across {} pages",
            pages.len()
        );
        for (index, page) in pages.iter().enumerate() {
            for rect in &page.rects {
                assert!(
                    rect.y + rect.height - page.top <= g.content_height() + SLACK,
                    "page {index} draws grid into the bottom margin"
                );
            }
            for run in &l.runs[page.runs.clone()] {
                let bottom = run.y + metrics::LINE_HEIGHT * run.size;
                assert!(
                    bottom - page.top <= g.content_height() + SLACK,
                    "page {index} puts a line into the bottom margin"
                );
            }
        }
    }

    #[test]
    fn consume_hands_over_emitted_math_and_the_drain_drops_it() {
        let mut source = String::new();
        for i in 0..40 {
            source.push_str(&format!("Filler paragraph number {i} with body.\n\n"));
            source.push_str("```math\n\\frac{a}{b}\n```\n\n");
        }
        let doc = markdown::parse(source.as_str());
        let mut l = laid_out(&doc);
        let total = l.math_glyphs.len();
        assert!(total > 0, "the fixture lays out math");
        let g = PageGeometry::new(PageSize::A4, Orientation::Portrait, 11.0);
        let mut paginator = Paginator::new();
        let pages = paginator.advance(&doc, &l, &g, true);
        assert!(pages.len() > 1, "the fixture spans pages");
        let carried: usize = pages.iter().map(|page| page.math.len()).sum();
        assert_eq!(carried, total, "every glyph lands on a page");
        let consumed = paginator.consume();
        assert_eq!(consumed.math, total, "emitted pages hand their math over");
        l.drain_front(
            consumed.runs,
            consumed.rects,
            consumed.images,
            consumed.math,
        );
        assert!(l.math_glyphs.is_empty(), "the drain drops what pages carry");
    }
}

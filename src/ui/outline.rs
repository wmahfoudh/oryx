//! Document outline: the pure tree behind the sidebar's Outline tab.
//! Entries mirror the model's heading blocks; a heading's parent is the
//! nearest preceding heading with a smaller level, so skipped levels
//! nest one visual step. Drawing and row geometry stay in the sidebar;
//! everything here is testable without a font system.

use std::collections::HashSet;

use crate::doc::model::{BlockKind, Document};

/// One heading of the document, in document order.
pub struct Entry {
    /// Display text, flattened from the heading's spans.
    pub text: String,
    /// Model block index; stable across document growth.
    pub block: usize,
    pub level: u8,
    /// Index of the parent entry.
    pub parent: Option<usize>,
    /// Tree depth, which drives the indent.
    pub depth: u8,
    /// Whether any entry names this one as parent.
    pub has_children: bool,
}

/// One visible row of the projection the sidebar draws.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Row {
    /// Index into `entries`.
    pub entry: usize,
    pub depth: u8,
    pub has_children: bool,
    pub collapsed: bool,
    /// A book TOC entry whose target does not resolve: drawn dimmed,
    /// jumps nowhere.
    pub dead: bool,
}

#[derive(Default)]
pub struct OutlineTree {
    entries: Vec<Entry>,
    /// Book mode: each entry's TOC target, parallel to `entries`, for
    /// resolving against the anchor map as deliveries land. Empty for
    /// the heading outline.
    targets: Vec<(String, Option<String>)>,
    /// Folded entries, keyed by model block index, which growth never
    /// shifts. Session-only; a rebuild starts expanded.
    collapsed: HashSet<usize>,
    /// Keyboard selection as an index into the current projection.
    pub selected: usize,
    /// List scroll offset in pixels; the sidebar clamps it.
    pub scroll: f32,
    /// Model blocks considered so far, the growth high-water mark.
    covered: usize,
    /// The entry last highlighted as current, for scroll-on-change.
    last_current: Option<usize>,
}

impl OutlineTree {
    /// Collects every heading of the document, folded state cleared.
    pub fn build(doc: &Document) -> OutlineTree {
        let mut tree = OutlineTree::default();
        tree.extend(doc);
        tree
    }

    /// A book's outline: the authored table of contents, nesting as
    /// written, every entry present from the start. Targets resolve
    /// through the anchor map to blocks; one the map does not cover yet
    /// stays unresolved until `re_resolve` after a delivery, and one
    /// naming an absent file stays unresolved for good.
    pub fn from_toc(toc: &[crate::doc::epub::TocEntry], doc: &Document) -> OutlineTree {
        let mut tree = OutlineTree::default();
        for entry in toc {
            // The heading machinery parents by level; a TOC's depth maps
            // onto it as level minus one.
            let parent = if entry.depth == 0 {
                None
            } else {
                tree.entries.iter().rposition(|e| e.level < entry.depth + 1)
            };
            let depth = parent.map(|p| tree.entries[p].depth + 1).unwrap_or(0);
            if let Some(p) = parent {
                tree.entries[p].has_children = true;
            }
            tree.entries.push(Entry {
                text: entry.label.clone(),
                block: usize::MAX,
                level: entry.depth + 1,
                parent,
                depth,
                has_children: false,
            });
            tree.targets
                .push((entry.path.clone(), entry.fragment.clone()));
        }
        tree.re_resolve(doc);
        tree
    }

    /// Resolves unresolved book entries against the anchor map; a
    /// delivery grows the map, so later chapters resolve here.
    pub fn re_resolve(&mut self, doc: &Document) {
        for (index, (path, fragment)) in self.targets.iter().enumerate() {
            if self.entries[index].block != usize::MAX || path.is_empty() {
                continue;
            }
            let offset = crate::doc::epub::resolve_target(doc, path, fragment.as_deref());
            if let Some(block) = offset.and_then(|o| doc.block_at_offset(o)) {
                self.entries[index].block = block;
            }
        }
    }

    /// Appends entries for blocks parsed since the last call. Existing
    /// entries, their indices, and the fold state stay untouched.
    pub fn extend(&mut self, doc: &Document) {
        for block in self.covered..doc.blocks.len() {
            let BlockKind::Heading { level, spans, .. } = &doc.blocks[block].kind else {
                continue;
            };
            // A book heading may break its title over lines with <br>;
            // the sidebar row draws one line.
            let text: String = spans.iter().map(|s| s.text(&doc.source)).collect();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let parent = self.entries.iter().rposition(|e| e.level < *level);
            let depth = parent.map(|p| self.entries[p].depth + 1).unwrap_or(0);
            if let Some(p) = parent {
                self.entries[p].has_children = true;
            }
            self.entries.push(Entry {
                text,
                block,
                level: *level,
                parent,
                depth,
                has_children: false,
            });
        }
        self.covered = doc.blocks.len();
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether every ancestor of an entry is unfolded.
    fn ancestors_open(&self, entry: usize) -> bool {
        let mut chain = self.entries[entry].parent;
        while let Some(p) = chain {
            if self.collapsed.contains(&self.entries[p].block) {
                return false;
            }
            chain = self.entries[p].parent;
        }
        true
    }

    /// The visible rows, in document order. A folded entry keeps its own
    /// row; its descendants leave.
    pub fn rows(&self) -> Vec<Row> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, _)| self.ancestors_open(*i))
            .map(|(i, e)| Row {
                entry: i,
                depth: e.depth,
                has_children: e.has_children,
                collapsed: self.collapsed.contains(&e.block),
                dead: e.block == usize::MAX,
            })
            .collect()
    }

    /// Folds or unfolds the entry behind a projection row. Rows without
    /// children have nothing to fold.
    pub fn toggle_row(&mut self, row: usize) {
        let rows = self.rows();
        let Some(row) = rows.get(row) else {
            return;
        };
        if !row.has_children {
            return;
        }
        let block = self.entries[row.entry].block;
        if !self.collapsed.remove(&block) {
            self.collapsed.insert(block);
        }
    }

    /// The current section: the last heading placed at or above the
    /// viewport top. `top` answers a heading block's document y, None
    /// while it is folded away or not yet placed.
    pub fn current_of(&self, scroll_y: f32, top: impl Fn(usize) -> Option<f32>) -> Option<usize> {
        self.entries
            .iter()
            .rposition(|e| top(e.block).is_some_and(|y| y <= scroll_y + 1.0))
    }

    /// The entry carrying the current highlight: the current section
    /// itself, or its deepest visible ancestor while it sits inside a
    /// folded branch.
    pub fn visible_entry(&self, entry: usize) -> usize {
        let mut candidate = entry;
        loop {
            if self.ancestors_open(candidate) {
                return candidate;
            }
            candidate = self.entries[candidate]
                .parent
                .expect("a folded chain has parents");
        }
    }

    /// Moves the keyboard selection over the projection.
    pub fn move_selection(&mut self, delta: i32, row_h: f32, list_h: f32) {
        let len = self.rows().len();
        if len == 0 {
            return;
        }
        let max = len as i64 - 1;
        self.selected = (self.selected as i64 + delta as i64).clamp(0, max) as usize;
        self.scroll_to_row(self.selected, row_h, list_h);
    }

    /// Scrolls just enough to keep one row inside the list.
    fn scroll_to_row(&mut self, row: usize, row_h: f32, list_h: f32) {
        let top = row as f32 * row_h;
        let list_h = list_h.max(row_h);
        if top < self.scroll {
            self.scroll = top;
        } else if top + row_h > self.scroll + list_h {
            self.scroll = top + row_h - list_h;
        }
    }

    /// Records the current entry, scrolling its row into view when the
    /// section changed; manual scrolling stays untouched otherwise.
    pub fn track_current(&mut self, current: Option<usize>, row_h: f32, list_h: f32) {
        if current == self.last_current {
            return;
        }
        self.last_current = current;
        let Some(entry) = current else {
            return;
        };
        let shown = self.visible_entry(entry);
        if let Some(row) = self.rows().iter().position(|r| r.entry == shown) {
            self.scroll_to_row(row, row_h, list_h);
        }
    }

    pub fn last_current(&self) -> Option<usize> {
        self.last_current
    }

    /// The heading block behind the keyboard selection.
    pub fn selected_block(&self) -> Option<usize> {
        let rows = self.rows();
        rows.get(self.selected).map(|r| self.entries[r.entry].block)
    }
}

/// The source offset an outline entry points at, or None when the
/// entry never resolved or the document does not hold that block. The
/// editor keeps the rendered page's outline while it shows the file's
/// source, so an entry's block index and the document on screen can
/// belong to different models; every lookup goes through here rather
/// than indexing a block table directly.
pub fn entry_offset(doc: &Document, block: usize) -> Option<usize> {
    doc.blocks.get(block).map(|b| b.range.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::markdown;

    fn doc(source: &str) -> Document {
        markdown::parse(source)
    }

    #[test]
    fn every_entry_points_at_its_heading_in_the_source() {
        let src = "# One\n\ntext\n\n## Two\n\nmore text here\n\n### Three\n\ntail\n";
        let d = doc(src);
        let tree = OutlineTree::build(&d);
        assert_eq!(tree.entries().len(), 3);
        for entry in tree.entries() {
            let offset = entry_offset(&d, entry.block).expect("the entry resolves");
            // A heading block's range opens at its text, past the
            // markers, so the entry lands the caret inside the heading.
            // What has to hold is the row: the line it stands on is the
            // heading's own.
            let line_start = src[..offset].rfind('\n').map_or(0, |i| i + 1);
            assert!(
                src[line_start..].starts_with('#'),
                "{:?} resolved to {offset}, which is not on a heading line",
                entry.text
            );
        }
    }

    #[test]
    fn an_entry_from_another_model_resolves_to_nothing() {
        // The editor keeps the page's outline while the source is on
        // screen, so a lookup can meet a document with one block where
        // the entry was numbered against many.
        let source_view = crate::doc::load::code_document(Some("md"), "# One\n\n## Two\n");
        assert_eq!(entry_offset(&source_view, 4), None);
        assert_eq!(entry_offset(&source_view, usize::MAX), None);
    }

    fn levels(tree: &OutlineTree) -> Vec<(u8, &str)> {
        tree.entries
            .iter()
            .map(|e| (e.depth, e.text.as_str()))
            .collect()
    }

    #[test]
    fn skipped_levels_nest_one_step() {
        let d = doc("## Alpha\n\n#### Beta\n\n### Gamma\n\n## Delta");
        let tree = OutlineTree::build(&d);
        assert_eq!(
            levels(&tree),
            vec![(0, "Alpha"), (1, "Beta"), (1, "Gamma"), (0, "Delta")]
        );
        assert_eq!(tree.entries[1].parent, Some(0), "h4 under the h2");
        assert_eq!(tree.entries[2].parent, Some(0), "h3 under the same h2");
        assert!(tree.entries[0].has_children);
        assert!(!tree.entries[3].has_children);
        let blocks: Vec<usize> = tree.entries.iter().map(|e| e.block).collect();
        assert_eq!(blocks, vec![0, 1, 2, 3], "entries key on model blocks");
    }

    #[test]
    fn a_fold_hides_children_and_keeps_siblings() {
        let d = doc("# A\n\n## B\n\n### C\n\n# D");
        let mut tree = OutlineTree::build(&d);
        assert_eq!(tree.rows().len(), 4);
        tree.toggle_row(0);
        let rows = tree.rows();
        assert_eq!(rows.len(), 2, "B and C leave, D stays");
        assert_eq!(rows[0].entry, 0);
        assert!(rows[0].collapsed);
        assert_eq!(rows[1].entry, 3);
        tree.toggle_row(0);
        assert_eq!(tree.rows().len(), 4, "unfolding restores");
    }

    #[test]
    fn a_row_without_children_does_not_fold() {
        let d = doc("# A\n\n# B");
        let mut tree = OutlineTree::build(&d);
        tree.toggle_row(0);
        assert_eq!(tree.rows().len(), 2);
    }

    #[test]
    fn collapse_survives_extension() {
        let head = "# A\n\n## B\n\nprose\n\n";
        let d1 = doc(head);
        let mut tree = OutlineTree::build(&d1);
        tree.toggle_row(0);
        assert_eq!(tree.rows().len(), 1);

        let d2 = doc(&format!("{head}## C\n\n# D"));
        tree.extend(&d2);
        let rows = tree.rows();
        assert_eq!(
            rows.iter().map(|r| r.entry).collect::<Vec<_>>(),
            vec![0, 3],
            "A stays folded, its new child hides, D appends"
        );
        assert_eq!(tree.entries()[2].parent, Some(0), "C nests under folded A");
    }

    #[test]
    fn current_at_the_boundaries() {
        let d = doc("# A\n\n# B\n\n# C");
        let tree = OutlineTree::build(&d);
        let tops = |block: usize| match block {
            0 => Some(100.0),
            1 => Some(500.0),
            2 => Some(900.0),
            _ => None,
        };
        assert_eq!(tree.current_of(0.0, tops), None, "above the first heading");
        assert_eq!(tree.current_of(100.0, tops), Some(0), "exactly at the top");
        assert_eq!(tree.current_of(600.0, tops), Some(1));
        assert_eq!(tree.current_of(5000.0, tops), Some(2), "past the last");
        let none = |_: usize| None;
        assert_eq!(tree.current_of(600.0, none), None, "nothing placed yet");
    }

    #[test]
    fn a_folded_current_highlights_its_deepest_visible_ancestor() {
        let d = doc("# A\n\n## B\n\n### C\n\n# D");
        let mut tree = OutlineTree::build(&d);
        tree.toggle_row(1);
        assert_eq!(tree.visible_entry(2), 1, "C folds under B");
        tree.toggle_row(0);
        assert_eq!(tree.visible_entry(2), 0, "outer fold wins");
        assert_eq!(tree.visible_entry(3), 3, "unfolded entries stand");
    }

    #[test]
    fn no_headings_means_no_rows() {
        let d = doc("just prose\n\nmore prose");
        let tree = OutlineTree::build(&d);
        assert!(tree.rows().is_empty());
    }

    #[test]
    fn tracking_scrolls_only_on_change() {
        let d = doc("# A\n\n# B\n\n# C\n\n# D\n\n# E");
        let mut tree = OutlineTree::build(&d);
        let (row_h, list_h) = (30.0, 60.0);
        tree.track_current(Some(4), row_h, list_h);
        let after_jump = tree.scroll;
        assert!(after_jump > 0.0, "the current row scrolled into view");
        tree.scroll = 0.0;
        tree.track_current(Some(4), row_h, list_h);
        assert_eq!(tree.scroll, 0.0, "an unchanged current leaves scroll be");
    }
}

//! The view while a big file is measured.
//!
//! A code or text file is one block, and its layout answers for a place
//! only once the measure has reached it: until then a jump to the end,
//! to a far line, or a zoom step far down the file shows an empty page.
//! The place the view is going to is known all the while, as a line and
//! where it stands in the window. A preview is the lines around that
//! line, laid out as a small document of their own by the ordinary
//! layout and painted in the page's place, without line numbers, as
//! the page has none while its block is measured. Nothing reads its positions
//! but its own paint: the file's layout, its measure and its landings
//! are as they are without it.

use std::ops::Range;
use std::sync::Arc;

use crate::doc::images::MediaCache;
use crate::doc::model::{Block, BlockKind, Document};
use crate::layout::{layout, LayoutDoc, ViewConfig};
use crate::style::fonts::FontStore;
use crate::style::theme::Theme;

/// Some lines of a file of one block, as a document and its layout.
/// Source offsets mean in it what they mean in the file, so a landing
/// is computed on it by the functions that compute it on the file.
pub struct Preview {
    pub doc: Document,
    pub lay: LayoutDoc,
    /// The rows of the file the preview holds.
    pub rows: Range<usize>,
    /// The source bytes it holds of its one row, when that row is a
    /// line longer than `LONG_LINE` shown by some of its pieces.
    pub piece: Option<Range<usize>>,
}

/// The rows a preview holds to show `row` anywhere in a window `view_h`
/// tall, each row `row_h` or more: two windows of rows above and three
/// below a start that moves a window at a time, so that the view can
/// scroll a little without a new preview.
pub fn rows_around(total: usize, row: usize, view_h: f32, row_h: f32) -> Range<usize> {
    let screen = (view_h / row_h.max(1.0)).ceil() as usize + 2;
    let row = row.min(total.saturating_sub(1));
    let from = row - row % screen;
    from.saturating_sub(2 * screen)..(from + 3 * screen).min(total)
}

/// The most text a preview lays out. Five windows of ordinary lines
/// are a few tens of kilobytes.
const MOST_BYTES: usize = 128 * 1024;

/// The preview of `rows` of `doc`, a code or text file whose one block
/// reads its lines in the source. None for any other document, and for
/// rows of more than `MOST_BYTES` of text.
pub fn build(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    width: f32,
    rows: Range<usize>,
) -> Option<Preview> {
    if !(doc.code_file || doc.plain_file) {
        return None;
    }
    let [block] = &doc.blocks[..] else {
        return None;
    };
    let BlockKind::CodeBlock {
        language,
        lines,
        highlights,
        ..
    } = &block.kind
    else {
        return None;
    };
    if !lines.is_verbatim() || rows.is_empty() || rows.end > lines.len() {
        return None;
    }
    // Each row is shaped here, on the window's thread, in one go: rows
    // of very long lines would hold the window longer than the empty
    // page they replace is worth.
    let bytes = lines.line_range(rows.end - 1)?.end - lines.line_range(rows.start)?.start;
    if bytes > MOST_BYTES {
        return None;
    }
    let colored = rows.start..rows.end.min(highlights.len());
    let part = Document {
        blocks: vec![Block {
            quote_depth: block.quote_depth,
            alert: block.alert,
            // The block's own range: an offset of the file finds the
            // block here as it does there.
            range: block.range.clone(),
            centered: block.centered,
            right: block.right,
            left: block.left,
            details: block.details,
            kind: BlockKind::CodeBlock {
                language: language.clone(),
                lines: lines.window(rows.clone()),
                highlights: highlights
                    .get(colored)
                    .map(<[_]>::to_vec)
                    .unwrap_or_default(),
                exact: 0,
            },
        }],
        source: Arc::clone(&doc.source),
        code_file: doc.code_file,
        plain_file: doc.plain_file,
        ..Document::default()
    };
    let lay = layout(&part, theme, fonts, media, cfg, width);
    Some(Preview {
        doc: part,
        lay,
        rows,
        piece: None,
    })
}

/// The preview of some pieces of row `row` of `doc`, a line longer than
/// `LONG_LINE`: the source bytes `bytes`, which start and end where
/// pieces of the line do (`scroll::pieces_around`). The layout cuts a
/// line from its start by its text, so the pieces of the preview are
/// those of the file. A file of one very long line has no rows around
/// a place to show, and its one row is far more than `MOST_BYTES`.
/// None for a document `build` refuses.
#[allow(clippy::too_many_arguments)]
pub fn build_pieces(
    doc: &Document,
    theme: &Theme,
    fonts: &mut FontStore,
    media: &mut MediaCache,
    cfg: &ViewConfig,
    width: f32,
    row: usize,
    bytes: Range<usize>,
) -> Option<Preview> {
    if !(doc.code_file || doc.plain_file) {
        return None;
    }
    let [block] = &doc.blocks[..] else {
        return None;
    };
    let BlockKind::CodeBlock {
        language, lines, ..
    } = &block.kind
    else {
        return None;
    };
    let line = lines.line_range(row)?;
    if !lines.is_verbatim()
        || bytes.is_empty()
        || bytes.len() > MOST_BYTES
        || bytes.start < line.start
        || bytes.end > line.end
    {
        return None;
    }
    let part = Document {
        blocks: vec![Block {
            quote_depth: block.quote_depth,
            alert: block.alert,
            range: block.range.clone(),
            centered: block.centered,
            right: block.right,
            left: block.left,
            details: block.details,
            kind: BlockKind::CodeBlock {
                language: language.clone(),
                // A long line has no colors.
                lines: lines.part(bytes.clone()),
                highlights: Vec::new(),
                exact: 0,
            },
        }],
        source: Arc::clone(&doc.source),
        code_file: doc.code_file,
        plain_file: doc.plain_file,
        ..Document::default()
    };
    let lay = layout(&part, theme, fonts, media, cfg, width);
    Some(Preview {
        doc: part,
        lay,
        rows: row..row + 1,
        piece: Some(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;
    use crate::paint::{band_numbered, scroll};
    use std::path::PathBuf;

    fn text() -> String {
        let mut text = String::new();
        for i in 0..3000 {
            match i % 9 {
                0 => text.push_str(&format!(
                    "let value_{i} = \"{}\";\n",
                    "word ".repeat(i % 40)
                )),
                1 => text.push_str(&format!("\tindented(with, a, tab, {i});\n")),
                2 => text.push_str(&format!(
                    "// caf\u{e9} {i} {}\n",
                    "\u{e9}t\u{e9} ".repeat(i % 30)
                )),
                3 => text.push('\n'),
                _ => text.push_str(&format!("fn f{i}() {{ call({i}, {}); }}\n", i * 7)),
            }
        }
        text
    }

    /// The page painted at the place of `row`, `below` under the top of
    /// a window of 1000 by 600, from a layout and its document.
    fn painted(
        lay: &LayoutDoc,
        doc: &Document,
        fonts: &mut FontStore,
        row: usize,
        below: f32,
    ) -> Vec<u32> {
        let theme = Theme::default_dark();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let top = lay.approx_top(0, row).unwrap();
        let y = scroll::clamp(top - below, lay.height, 600.0);
        band_numbered(
            lay,
            doc,
            &theme,
            fonts,
            &mut media,
            &[],
            None,
            scroll::frame_offset(y),
            1000,
            600,
        )
    }

    #[test]
    fn a_preview_paints_what_the_settled_page_paints_at_the_same_place() {
        let dir = std::env::temp_dir().join("oryx-preview");
        std::fs::create_dir_all(&dir).unwrap();
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        for name in ["preview.rs", "preview.txt"] {
            let path = dir.join(name);
            std::fs::write(&path, text()).unwrap();
            let doc = load::open(&path, None).unwrap().document;
            let cfg = ViewConfig::default();
            let full = layout(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0);
            let row_h =
                crate::layout::metrics::LINE_HEIGHT * crate::layout::line_face(&doc, &cfg).1;
            for row in [0, 37, 1500, 1503, 2990, 2999] {
                let rows = rows_around(3000, row, 600.0, row_h);
                assert!(rows.contains(&row), "{rows:?} holds {row}");
                let preview = build(
                    &doc,
                    &theme,
                    &mut fonts,
                    &mut media,
                    &cfg,
                    1000.0,
                    rows.clone(),
                )
                .unwrap();
                assert_eq!(preview.rows, rows);
                for below in [0.0, 137.0, 570.0] {
                    let page = painted(&full, &doc, &mut fonts, row, below);
                    let part = painted(
                        &preview.lay,
                        &preview.doc,
                        &mut fonts,
                        row - rows.start,
                        below,
                    );
                    let differ = page.iter().zip(&part).filter(|(a, b)| a != b).count();
                    assert_eq!(differ, 0, "{name} row {row} below {below}");
                }
            }
        }
    }

    /// A file of one line of 400 KB: the preview of four pieces of it
    /// paints what the file's own layout paints at the same place, at
    /// the top of a piece and inside one.
    #[test]
    fn a_preview_of_pieces_paints_what_the_settled_page_paints_there() {
        let dir = std::env::temp_dir().join("oryx-preview-pieces");
        std::fs::create_dir_all(&dir).unwrap();
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let cfg = ViewConfig::default();
        let line: String = (0..60_000).map(|i| format!("word{i} ")).collect();
        for name in ["pieces.json", "pieces.txt"] {
            let path = dir.join(name);
            std::fs::write(&path, format!("{line}\n")).unwrap();
            let doc = load::open(&path, None).unwrap().document;
            let full = layout(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0);
            assert!(build(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0, 0..1).is_none());
            for at in [0, 9_000, line.len() / 2, line.len() - 5] {
                let bytes = scroll::pieces_around(&full, &doc, at, 1, 2).expect("a long line");
                assert!(bytes.contains(&at) && bytes.len() <= 4 * 8192, "{bytes:?}");
                let preview = build_pieces(
                    &doc,
                    &theme,
                    &mut fonts,
                    &mut media,
                    &cfg,
                    1000.0,
                    0,
                    bytes.clone(),
                )
                .unwrap();
                assert_eq!(preview.piece, Some(bytes.clone()));
                for below in [0.0_f32, -300.0, -1500.0] {
                    // The same place on both: so far under the top of
                    // the piece holding `at`.
                    let place = |lay: &LayoutDoc, doc: &Document| {
                        let top = scroll::offset_top(lay, doc, at).unwrap();
                        scroll::frame_offset(scroll::clamp(top - below, lay.height, 600.0))
                    };
                    let mut paint = |lay: &LayoutDoc, doc: &Document| {
                        let y = place(lay, doc);
                        band_numbered(
                            lay,
                            doc,
                            &theme,
                            &mut fonts,
                            &mut media,
                            &[],
                            None,
                            y,
                            1000,
                            600,
                        )
                    };
                    let page = paint(&full, &doc);
                    let part = paint(&preview.lay, &preview.doc);
                    // The pages differ where the view passes the pieces
                    // the preview holds; these places do not.
                    let top = scroll::offset_top(&preview.lay, &preview.doc, at).unwrap() - below;
                    if top + 600.0 <= preview.lay.height && top >= 0.0 {
                        let differ = page.iter().zip(&part).filter(|(a, b)| a != b).count();
                        assert_eq!(differ, 0, "{name} at {at} below {below}");
                    }
                }
            }
        }
    }

    #[test]
    fn rows_of_very_long_lines_have_no_preview() {
        let dir = std::env::temp_dir().join("oryx-preview-long");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.rs");
        let line = format!("{}\n", "x".repeat(40_000));
        std::fs::write(&path, line.repeat(8)).unwrap();
        let doc = load::open(&path, None).unwrap().document;
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let cfg = ViewConfig::default();
        assert!(build(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0, 0..8).is_none());
        assert!(build(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0, 0..3).is_some());
    }

    #[test]
    fn a_page_of_several_blocks_has_no_preview() {
        let theme = Theme::default_dark();
        let mut fonts = FontStore::new();
        let mut media = MediaCache::offline(PathBuf::from("."));
        let doc = crate::doc::markdown::parse("# Title\n\ntext\n\n```\ncode\n```\n");
        let cfg = ViewConfig::default();
        assert!(build(&doc, &theme, &mut fonts, &mut media, &cfg, 1000.0, 0..1).is_none());
    }

    #[test]
    fn the_rows_of_a_preview_stay_the_same_while_the_row_moves_a_little() {
        let a = rows_around(100_000, 5000, 600.0, 30.0);
        let b = rows_around(100_000, 5003, 600.0, 30.0);
        assert_eq!(a, b);
        assert!(a.start <= 5000 - 22 && a.end >= 5003 + 22);
        assert_eq!(rows_around(10, 9, 600.0, 30.0), 0..10);
        assert_eq!(rows_around(0, 0, 600.0, 30.0), 0..0);
    }
}

//! Find in document: smart-case substring matching over the layout's
//! visual lines. Each match is a `Selection`, so highlight geometry and
//! scroll targets reuse the selection machinery.

use std::ops::Range;

use crate::doc::model::Document;
use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{Rgba, Theme};
use crate::ui::selection::{block_pieces, ModelPos, Piece, Selection};
use crate::ui::textfield::TextField;

/// Run index marking a boundary character no match may cover.
/// A character no query can contain, planted between blocks so a match
/// never crosses them.
const BLOCK_SEP: char = '\u{1}';

const BAR_WIDTH: f32 = 280.0;
const BAR_HEIGHT: f32 = 40.0;
const MARGIN: f32 = 16.0;
const PAD: f32 = 16.0;
const RADIUS: f32 = 20.0;
const QUERY_SIZE: f32 = 15.0;
const COUNTER_SIZE: f32 = 13.0;

/// Live find session: the query as typed, its matches, and the cursor
/// among them. `stale` marks the matches for recomputation against the
/// current layout on the next frame; `rects` caches every match's
/// highlight boxes with their match index, so painting a frame never
/// re-shapes text. The query is a `TextField`, so reopening the bar with
/// the old query selected is a plain `select_all` and the next typed
/// character replaces it.
pub struct SearchState {
    pub query: TextField,
    pub matches: Vec<Selection>,
    /// Highlight rects for the matches inside the band window alone;
    /// the counter and navigation use the full match list.
    pub rects: Vec<(usize, (f32, f32, f32, f32))>,
    /// The scroll position `rects` was computed around, so drifting a
    /// viewport past it triggers a refresh.
    pub rects_scroll: f32,
    pub current: usize,
    pub stale: bool,
    /// The current match landed on its recorded block top because its
    /// region was cold; the exact anchor still owes a centering.
    pub settle: bool,
}

/// The floating pill over the document's top-right corner: query on the
/// left, match counter on the right, in the theme's overlay colors.
pub fn draw_bar(painter: &mut Painter, theme: &Theme, state: &SearchState, width: f32) {
    let ui = &theme.ui;
    let x = (width - BAR_WIDTH - MARGIN).max(MARGIN);
    let y = MARGIN;
    for (grow, alpha) in [(6.0, 16), (3.0, 28)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            BAR_WIDTH + 2.0 * grow,
            BAR_HEIGHT + 2.0 * grow,
            RADIUS + grow,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: alpha,
            },
        );
    }
    painter.fill(x, y, BAR_WIDTH, BAR_HEIGHT, RADIUS, ui.overlay_bg);
    painter.stroke(
        x,
        y,
        BAR_WIDTH,
        BAR_HEIGHT,
        RADIUS,
        1.0,
        theme.blocks.table_border,
    );

    let counter = if state.query.is_empty() {
        String::new()
    } else {
        let shown = if state.matches.is_empty() {
            0
        } else {
            state.current + 1
        };
        format!("{shown}/{}", state.matches.len())
    };
    let counter_w = painter.measure(&counter, CODE_FAMILY, COUNTER_SIZE, 400);
    let counter_color = if !state.query.is_empty() && state.matches.is_empty() {
        theme.alerts.caution
    } else {
        theme.blocks.frontmatter_fg
    };
    // Tops differ by the ascent difference so both texts share a baseline.
    painter.text(
        x + BAR_WIDTH - PAD - counter_w,
        y + 10.0 + (QUERY_SIZE - COUNTER_SIZE) * 1.1,
        &counter,
        CODE_FAMILY,
        COUNTER_SIZE,
        400,
        counter_color,
    );

    let avail = BAR_WIDTH - 2.0 * PAD - counter_w - 12.0;
    let query = state.query.text();
    let (window, cut) = window_fit(painter, query, state.query.caret(), avail);
    let shown = if cut {
        format!("\u{2026}{}", &query[window.clone()])
    } else {
        query[window.clone()].to_string()
    };
    let lead = if cut {
        painter.measure("\u{2026}", BODY_FAMILY, QUERY_SIZE, 400)
    } else {
        0.0
    };
    // Offsets are taken inside the drawn window, so a caret in the middle
    // of a query too long for the pill still lands under its character.
    let x_of = |painter: &mut Painter, at: usize| {
        let at = at.clamp(window.start, window.end);
        lead + painter.measure(&query[window.start..at], BODY_FAMILY, QUERY_SIZE, 400)
    };
    if let Some(range) = state.query.selection() {
        let from = x_of(painter, range.start);
        let to = x_of(painter, range.end);
        painter.fill(
            x + PAD + from - 2.0,
            y + 9.0,
            to - from + 4.0,
            BAR_HEIGHT - 18.0,
            4.0,
            ui.selection_bg,
        );
    }
    let caret_x = x + PAD + x_of(painter, state.query.caret()) + 1.0;
    if query.is_empty() {
        painter.text(
            x + PAD + 6.0,
            y + 10.0,
            "find",
            BODY_FAMILY,
            QUERY_SIZE,
            400,
            theme.blocks.frontmatter_fg,
        );
    } else {
        painter.text(
            x + PAD,
            y + 10.0,
            &shown,
            BODY_FAMILY,
            QUERY_SIZE,
            400,
            ui.overlay_fg,
        );
    }
    painter.line(
        caret_x,
        y + 11.0,
        caret_x,
        y + BAR_HEIGHT - 11.0,
        1.0,
        ui.overlay_fg,
    );
}

/// The slice of a long query that gets drawn, as a byte range that always
/// contains the caret, and whether characters were cut from the left. The
/// tail is preferred, since that is where typing happens; a caret left of
/// the tail window anchors the window on itself instead.
fn window_fit(
    painter: &mut Painter,
    query: &str,
    caret: usize,
    avail: f32,
) -> (Range<usize>, bool) {
    let fits = |painter: &mut Painter, text: &str| {
        painter.measure(text, BODY_FAMILY, QUERY_SIZE, 400) <= avail
    };
    if fits(painter, query) {
        return (0..query.len(), false);
    }
    let mut start = query.len();
    for (index, _) in query.char_indices().skip(1) {
        if fits(painter, &format!("\u{2026}{}", &query[index..])) {
            start = index;
            break;
        }
    }
    if start <= caret {
        return (start..query.len(), true);
    }
    let mut end = caret;
    for (offset, c) in query[caret..].char_indices() {
        let candidate = caret + offset + c.len_utf8();
        if !fits(painter, &format!("\u{2026}{}", &query[caret..candidate])) {
            break;
        }
        end = candidate;
    }
    (caret..end, caret > 0)
}

/// A query with any capital letter matches exactly; an all-lowercase
/// query matches case-insensitively.
pub fn smart_case_sensitive(query: &str) -> bool {
    query.chars().any(|c| c.is_uppercase())
}

/// Lowercase fold for matching. A multi-character expansion keeps only
/// its first character so haystack and needle stay aligned one to one.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// One searchable character: its model position (None for the block
/// separators and display joiners), the character, and its byte length
/// for computing end positions.
struct HChar {
    pos: Option<ModelPos>,
    c: char,
    len: usize,
}

/// Every match in document order, found in the model's display text, so
/// matching needs no layout. Display joiners (tabs between table cells,
/// newlines between rows and code lines) keep a phrase from matching
/// across them, and blocks never join. Empty query, no matches.
pub fn matches(doc: &Document, query: &str) -> Vec<Selection> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = smart_case_sensitive(query);
    let needle: Vec<char> = if sensitive {
        query.chars().collect()
    } else {
        query.chars().map(fold).collect()
    };
    let hay = haystack(doc);
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let window = &hay[i..i + needle.len()];
        let hit = window.iter().zip(&needle).all(|(h, want)| {
            h.c != BLOCK_SEP && (if sensitive { h.c } else { fold(h.c) }) == *want
        });
        if hit {
            let start = window.iter().find_map(|h| h.pos);
            let end = window.iter().rev().find_map(|h| {
                h.pos.map(|p| ModelPos {
                    byte: p.byte + h.len,
                    ..p
                })
            });
            if let (Some(start), Some(end)) = (start, end) {
                out.push(Selection { start, end });
            }
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// The document's display text as one character sequence, walked from
/// the model through the same pieces the copies use.
fn haystack(doc: &Document) -> Vec<HChar> {
    let mut out: Vec<HChar> = Vec::new();
    for index in 0..doc.blocks.len() {
        let mut opened = false;
        for piece in block_pieces(doc, index) {
            match piece {
                Piece::Sep(sep) => {
                    for c in sep.chars() {
                        push_char(&mut out, &mut opened, None, c);
                    }
                }
                Piece::Label(label) => {
                    for c in label.chars() {
                        push_char(&mut out, &mut opened, None, c);
                    }
                }
                Piece::Addr { span, text } => {
                    let mut byte = 0usize;
                    for c in text.chars() {
                        let pos = ModelPos {
                            block: index,
                            span,
                            byte,
                        };
                        push_char(&mut out, &mut opened, Some(pos), c);
                        byte += c.len_utf8();
                    }
                }
            }
        }
    }
    out
}

/// Appends one character, planting the block separator ahead of a
/// block's first character.
fn push_char(out: &mut Vec<HChar>, opened: &mut bool, pos: Option<ModelPos>, c: char) {
    if !*opened {
        if !out.is_empty() {
            out.push(HChar {
                pos: None,
                c: BLOCK_SEP,
                len: 0,
            });
        }
        *opened = true;
    }
    out.push(HChar {
        pos,
        c,
        len: c.len_utf8(),
    });
}

/// The neighboring match index with wraparound in either direction.
pub fn step(current: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::markdown;
    use crate::ui::selection::ModelPos;

    #[test]
    fn lowercase_query_matches_any_case() {
        assert!(!smart_case_sensitive("panel"));
        assert!(!smart_case_sensitive("3/17"));
        let doc = markdown::parse("Panel PANEL panel");
        assert_eq!(matches(&doc, "panel").len(), 3);
    }

    #[test]
    fn capital_query_matches_exactly() {
        assert!(smart_case_sensitive("Panel"));
        let doc = markdown::parse("Panel PANEL panel");
        let found = matches(&doc, "Panel");
        assert_eq!(
            found,
            vec![Selection {
                start: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 0
                },
                end: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 5
                },
            }]
        );
    }

    #[test]
    fn match_crosses_a_style_boundary() {
        let doc = markdown::parse("**pan**el rest");
        let found = matches(&doc, "panel");
        assert_eq!(
            found,
            vec![Selection {
                start: ModelPos {
                    block: 0,
                    span: 0,
                    byte: 0
                },
                end: ModelPos {
                    block: 0,
                    span: 1,
                    byte: 2
                },
            }]
        );
    }

    #[test]
    fn no_match_across_blocks() {
        let doc = markdown::parse("one\n\ntwo");
        assert!(matches(&doc, "onetwo").is_empty());
        assert!(matches(&doc, "netw").is_empty());
    }

    #[test]
    fn no_match_across_table_cells() {
        let doc = markdown::parse("| ab | cd |\n|---|---|\n| ef | gh |");
        assert!(matches(&doc, "bc").is_empty());
        assert!(matches(&doc, "fg").is_empty());
        assert_eq!(matches(&doc, "ef").len(), 1);
    }

    #[test]
    fn no_match_across_a_hard_break() {
        let doc = markdown::parse("one two\\\nthree");
        assert!(matches(&doc, "two three").is_empty());
        assert_eq!(matches(&doc, "three").len(), 1);
    }

    #[test]
    fn markers_stay_out_of_the_text() {
        let doc = markdown::parse("- item one");
        let found = matches(&doc, "item");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].start,
            ModelPos {
                block: 0,
                span: 0,
                byte: 0
            },
            "the bullet is not part of the text"
        );
    }

    #[test]
    fn matches_span_code_lines_but_not_line_breaks() {
        let doc = markdown::parse("```rust\nlet alpha = 1;\nlet beta = 2;\n```");
        assert_eq!(matches(&doc, "alpha").len(), 1);
        assert!(matches(&doc, "1;let").is_empty(), "lines never join");
    }

    #[test]
    fn successive_matches_do_not_overlap() {
        let doc = markdown::parse("aaaa");
        assert_eq!(matches(&doc, "aa").len(), 2);
    }

    #[test]
    fn empty_query_finds_nothing() {
        let doc = markdown::parse("anything");
        assert!(matches(&doc, "").is_empty());
    }

    #[test]
    fn step_wraps_both_ways() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 1, true), 0);
    }
}

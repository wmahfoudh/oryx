//! Find in document: smart-case substring matching over the layout's
//! visual lines. Each match is a `Selection`, so highlight geometry and
//! scroll targets reuse the selection machinery.

use std::ops::Range;

use crate::layout::{LayoutDoc, TextRun};
use crate::paint::painter::Painter;
use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};
use crate::style::theme::{Rgba, Theme};
use crate::ui::selection::{RunPos, Selection, MARKER_SPAN};
use crate::ui::textfield::TextField;

/// Run index marking a boundary character no match may cover.
const SEPARATOR: usize = usize::MAX;

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
    pub rects: Vec<(usize, (f32, f32, f32, f32))>,
    pub current: usize,
    pub stale: bool,
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

/// One searchable character and the run position it came from.
struct Pos {
    run: usize,
    ch: usize,
    c: char,
}

/// Every match in layout order. Runs concatenate per visual line so a
/// match crosses style boundaries; lines, blocks, table cells, and gaps
/// left by inline images never join. Empty query, no matches.
pub fn matches(lay: &LayoutDoc, query: &str) -> Vec<Selection> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = smart_case_sensitive(query);
    let needle: Vec<char> = if sensitive {
        query.chars().collect()
    } else {
        query.chars().map(fold).collect()
    };
    let hay = haystack(lay);
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let window = &hay[i..i + needle.len()];
        let hit = window.iter().zip(&needle).all(|(pos, want)| {
            pos.run != SEPARATOR && (if sensitive { pos.c } else { fold(pos.c) }) == *want
        });
        if hit {
            let (first, last) = (&window[0], &window[needle.len() - 1]);
            out.push(Selection {
                start: RunPos {
                    run: first.run,
                    ch: first.ch,
                },
                end: RunPos {
                    run: last.run,
                    ch: last.ch + 1,
                },
            });
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// The document's text as one character sequence. A separator lands
/// between visual lines, at table cell boundaries (span numbering
/// restarts per cell, mirroring the selection separator rule), and
/// across horizontal gaps such as inline images.
fn haystack(lay: &LayoutDoc) -> Vec<Pos> {
    let mut out = Vec::new();
    let mut prev: Option<&TextRun> = None;
    for (index, run) in lay.runs.iter().enumerate() {
        if run.span == MARKER_SPAN {
            continue;
        }
        if let Some(p) = prev {
            let same_line = p.block == run.block && p.y == run.y;
            let boundary = !same_line || run.span <= p.span || run.x > p.x + p.width + 1.0;
            if boundary {
                out.push(Pos {
                    run: SEPARATOR,
                    ch: 0,
                    c: '\t',
                });
            }
        }
        for (ch, c) in run.text.chars().enumerate() {
            out.push(Pos { run: index, ch, c });
        }
        prev = Some(run);
    }
    out
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
    use crate::doc::images::MediaCache;
    use crate::doc::markdown;
    use crate::layout::{layout, ViewConfig};
    use crate::style::fonts::FontStore;
    use crate::style::theme::Theme;
    use crate::ui::selection::RunPos;
    use std::path::PathBuf;

    fn lay(source: &str) -> LayoutDoc {
        let doc = markdown::parse(source);
        let mut fonts = FontStore::new();
        let mut media = MediaCache::new(PathBuf::from("."));
        layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            2000.0,
        )
    }

    #[test]
    fn lowercase_query_matches_any_case() {
        assert!(!smart_case_sensitive("panel"));
        assert!(!smart_case_sensitive("3/17"));
        let l = lay("Panel PANEL panel");
        assert_eq!(matches(&l, "panel").len(), 3);
    }

    #[test]
    fn capital_query_matches_exactly() {
        assert!(smart_case_sensitive("Panel"));
        let l = lay("Panel PANEL panel");
        let found = matches(&l, "Panel");
        assert_eq!(
            found,
            vec![Selection {
                start: RunPos { run: 0, ch: 0 },
                end: RunPos { run: 0, ch: 5 },
            }]
        );
    }

    #[test]
    fn match_crosses_a_style_boundary() {
        let l = lay("**pan**el rest");
        assert_eq!(l.runs.len(), 2, "bold and plain runs expected");
        let found = matches(&l, "panel");
        assert_eq!(
            found,
            vec![Selection {
                start: RunPos { run: 0, ch: 0 },
                end: RunPos { run: 1, ch: 2 },
            }]
        );
    }

    #[test]
    fn no_match_across_blocks() {
        let l = lay("one\n\ntwo");
        assert!(matches(&l, "onetwo").is_empty());
        assert!(matches(&l, "netw").is_empty());
    }

    #[test]
    fn no_match_across_table_cells() {
        let l = lay("| ab | cd |\n|---|---|\n| ef | gh |");
        assert!(matches(&l, "bc").is_empty());
        assert!(matches(&l, "fg").is_empty());
        assert_eq!(matches(&l, "ef").len(), 1);
    }

    #[test]
    fn no_match_across_a_hard_break() {
        let l = lay("one two\\\nthree");
        assert!(matches(&l, "two three").is_empty());
        assert_eq!(matches(&l, "three").len(), 1);
    }

    #[test]
    fn marker_runs_stay_out_of_the_text() {
        let l = lay("- item one");
        let found = matches(&l, "item");
        assert_eq!(found.len(), 1);
        let run = found[0].start.run;
        assert!(l.runs[run].text.starts_with("item"));
        assert_eq!(found[0].start.ch, 0);
    }

    #[test]
    fn successive_matches_do_not_overlap() {
        let l = lay("aaaa");
        assert_eq!(matches(&l, "aa").len(), 2);
    }

    #[test]
    fn empty_query_finds_nothing() {
        let l = lay("anything");
        assert!(matches(&l, "").is_empty());
    }

    #[test]
    fn step_wraps_both_ways() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 1, true), 0);
    }
}

//! The other occurrences of the selected text, lit in the search's
//! match color while the selection stands: a double-clicked word, whole,
//! or a text on one line selected by a drag, a Shift+click or the
//! keyboard, wherever it appears. The set is found once per selection,
//! when it has settled, and lives exactly as long as that selection.

use crate::doc::model::{BlockKind, Document};
use crate::ui::search;
use crate::ui::selection::{self, ModelPos, Selection};

/// How a selection's occurrences are matched: a double-clicked word
/// whole, as `search::word_matches` finds it; a selected text as it is,
/// partial words included, as `search::text_matches` finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Word,
    Text,
}

pub struct Occurrences {
    /// The selection as made, a double-clicked word or a selected text.
    /// The set is dropped the moment the standing selection is no
    /// longer this one.
    pub word: Selection,
    /// How the matches were found, for a second search in the same
    /// text after a highlight fold.
    pub matched: Match,
    /// Every other match, in document order.
    pub matches: Vec<Selection>,
    /// The rectangles of the matches around the view, and the scroll
    /// they were computed at; the same windowing as the search's.
    pub rects: Vec<(f32, f32, f32, f32)>,
    pub rects_scroll: f32,
    /// The rectangles no longer fit the layout.
    pub stale: bool,
}

/// The length a selection may have and still light, in characters:
/// VS Code's default limit.
const LONGEST: usize = 200;

/// Whether a selected text lights its other occurrences: two characters
/// or more, no line break, not whitespace alone, at most `LONGEST`
/// characters. A single character and a selection across lines light
/// nothing, as in VS Code and Zed.
pub fn lights(text: &str) -> bool {
    let chars = text.chars().count();
    (2..=LONGEST).contains(&chars) && !text.contains('\n') && !text.chars().all(char::is_whitespace)
}

/// Whether a selection may light, read on its positions before its
/// text is built. A selection across blocks holds a line break, unless
/// its text lies in one of them: dragged from a block's end into the
/// next, or from a block to the next one's start, the break is at an
/// end and trimmed away, so the reach is read on the block that holds
/// the text. Within a block: one span, or not longer in bytes than
/// four times the character limit, which no text of that many
/// characters fits. The text of a selection of the whole page cost
/// 69 ms on an 8 MB page, built before the limit was read.
pub fn within_reach(doc: &Document, selection: &Selection) -> bool {
    let (mut start, mut end) = selection.ordered();
    if start.block + 1 == end.block && at_block_end(doc, start) {
        start = ModelPos {
            block: end.block,
            span: 0,
            byte: 0,
        };
    }
    if start.block + 1 == end.block && end.span == 0 && end.byte == 0 {
        if let Some(last) = block_end(doc, start.block) {
            end = last;
        }
    }
    start.block == end.block
        && (start.span != end.span || start.byte.abs_diff(end.byte) <= 4 * LONGEST)
}

/// The position after the last character of a block's text, for the
/// kinds that hold spans. None for the others.
fn block_end(doc: &Document, block: usize) -> Option<ModelPos> {
    let spans = match &doc.blocks.get(block)?.kind {
        BlockKind::Paragraph { spans }
        | BlockKind::Heading { spans, .. }
        | BlockKind::ListItem { spans, .. }
        | BlockKind::FootnoteDef { spans, .. }
        | BlockKind::Summary { spans, .. } => spans,
        _ => return None,
    };
    let last = spans.last()?;
    Some(ModelPos {
        block,
        span: spans.len() - 1,
        byte: last.text(&doc.source).len(),
    })
}

/// Whether no text of its block follows the position.
fn at_block_end(doc: &Document, pos: ModelPos) -> bool {
    block_end(doc, pos.block)
        .is_some_and(|end| pos.span > end.span || (pos.span == end.span && pos.byte >= end.byte))
}

/// The other occurrences of a selection: a double-clicked word, whole,
/// or a selected text trimmed of whitespace at its ends, wherever it
/// appears. None when the text does not light or stands nowhere else.
/// The set keeps the selection as made, since it lives as long as that
/// selection stands. A selection beyond reach on its positions is
/// refused before its text is built, which costs the blocks it spans.
pub fn find(doc: &Document, selection: Selection, matched: Match) -> Option<Occurrences> {
    if !within_reach(doc, &selection) {
        return None;
    }
    let text = selection::plain_text(&selection, doc);
    let text = text.trim();
    if !lights(text) {
        return None;
    }
    let (start, end) = selection.ordered();
    let found = match matched {
        Match::Word => search::word_matches(doc, text),
        Match::Text => search::text_matches(doc, text),
    };
    // A word styled in two spans is addressed differently by the click
    // and by the search, so the selected one is told by overlap.
    let matches: Vec<Selection> = found
        .into_iter()
        .filter(|m| {
            let (a, b) = m.ordered();
            b <= start || a >= end
        })
        .collect();
    (!matches.is_empty()).then_some(Occurrences {
        word: selection,
        matched,
        matches,
        rects: Vec::new(),
        rects_scroll: 0.0,
        stale: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;
    use crate::doc::markdown;
    use crate::ui::selection::{word_at, ModelPos};

    fn word(doc: &Document, block: usize, span: usize, byte: usize) -> Selection {
        word_at(doc, ModelPos { block, span, byte }).expect("a word under the click")
    }

    #[test]
    fn a_word_lights_and_a_letter_or_a_gap_does_not() {
        assert!(lights("panel"));
        assert!(lights("snake_case"));
        assert!(lights("x2"));
        assert!(lights("été"));
        assert!(!lights("a"), "a single character");
        assert!(!lights("é"), "one character of two bytes");
        assert!(!lights("   "));
        assert!(!lights(""));
    }

    /// A selection is refused on its positions before its text is
    /// built: across blocks, or longer in bytes within one span than any
    /// text of the character limit can be.
    #[test]
    fn a_selection_across_blocks_or_over_the_limit_is_refused_on_its_positions() {
        let doc = markdown::parse("alpha beta\n\ngamma delta");
        let at = |block, span, byte| ModelPos { block, span, byte };
        let across = Selection {
            start: at(0, 0, 0),
            end: at(1, 0, 3),
        };
        assert!(
            !within_reach(&doc, &across),
            "across two blocks, text in both"
        );
        let long = Selection {
            start: at(0, 0, 0),
            end: at(0, 0, 4 * LONGEST + 1),
        };
        assert!(!within_reach(&doc, &long), "over the limit in one span");
        let upward = Selection {
            start: at(0, 0, 4 * LONGEST + 1),
            end: at(0, 0, 0),
        };
        assert!(!within_reach(&doc, &upward), "the same, dragged upward");
        let short = Selection {
            start: at(0, 0, 0),
            end: at(0, 0, 5),
        };
        assert!(within_reach(&doc, &short));
        let spans = Selection {
            start: at(0, 0, 0),
            end: at(0, 2, 5),
        };
        assert!(
            within_reach(&doc, &spans),
            "across spans of one block: the text decides"
        );
    }

    /// A selection across two blocks whose text lies in one of them
    /// lights: dragged from a paragraph's end into the next, its text
    /// is the next block's, trimmed of the break, and the reach is read
    /// there (the hunt's claim 31).
    #[test]
    fn a_selection_from_a_block_end_into_the_next_lights_on_its_text() {
        let doc = markdown::parse("The panel opens.\n\npanel, another panel.");
        let at = |block, span, byte| ModelPos { block, span, byte };
        let from_end = Selection {
            start: at(0, 0, 16),
            end: at(1, 0, 5),
        };
        assert_eq!(selection::plain_text(&from_end, &doc).trim(), "panel");
        assert!(within_reach(&doc, &from_end));
        let lit = find(&doc, from_end, Match::Text).expect("lit on the next block's word");
        assert_eq!(lit.matches.len(), 2, "the two other panels");
        let to_start = Selection {
            start: at(0, 0, 4),
            end: at(1, 0, 0),
        };
        assert!(within_reach(&doc, &to_start), "into the next block's start");
        let both = Selection {
            start: at(0, 0, 4),
            end: at(1, 0, 5),
        };
        assert!(!within_reach(&doc, &both), "text in both blocks");
    }

    #[test]
    fn the_other_occurrences_are_found_and_the_clicked_word_left_out() {
        let doc = markdown::parse("The panel opens.\n\nA panel, another panel and a Panel.");
        let clicked = word(&doc, 1, 0, 3);
        assert_eq!(selection::plain_text(&clicked, &doc), "panel");
        let found = find(&doc, clicked, Match::Word).expect("two other places");
        assert_eq!(found.word, clicked);
        let texts: Vec<String> = found
            .matches
            .iter()
            .map(|m| selection::plain_text(m, &doc))
            .collect();
        assert_eq!(texts, vec!["panel", "panel"], "exact case only");
        assert!(!found.matches.contains(&clicked));
        assert_eq!(found.matches[0].start.block, 0);
        assert!(found.stale, "the rectangles wait for the first frame");
    }

    #[test]
    fn a_selection_of_several_words_lights_and_a_line_break_or_spaces_do_not() {
        assert!(lights("the quick brown"));
        assert!(lights("--"), "marks light too, as in VS Code and Zed");
        assert!(lights("a."), "two characters");
        assert!(!lights("the\nquick"), "a line break");
        assert!(!lights(" \t"), "whitespace alone");
        assert!(lights(&"x".repeat(200)), "200 characters, the limit");
        assert!(!lights(&"x".repeat(201)), "over the limit");
    }

    #[test]
    fn a_sentence_lights_its_other_occurrences_partial_words_included() {
        let doc = markdown::parse(
            "the quick brown fox and the quick brown dog, but the quickest cat and the quick brown.\n\nAnother the quick brown here.",
        );
        let phrase = |start: usize, end: usize| Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: start,
            },
            end: ModelPos {
                block: 0,
                span: 0,
                byte: end,
            },
        };
        let sel = phrase(0, 15);
        assert_eq!(selection::plain_text(&sel, &doc), "the quick brown");
        let found = find(&doc, sel, Match::Text).expect("three other places");
        assert_eq!(found.word, sel);
        let texts: Vec<String> = found
            .matches
            .iter()
            .map(|m| selection::plain_text(m, &doc))
            .collect();
        assert_eq!(
            texts,
            vec!["the quick brown", "the quick brown", "the quick brown"]
        );
        assert!(
            !found.matches.contains(&sel),
            "the selection itself is left out"
        );
        let found = find(&doc, phrase(0, 9), Match::Text).expect("four other places");
        assert_eq!(found.matches.len(), 4, "\"the quickest\" lights too");
        let found = find(&doc, phrase(3, 16), Match::Text).expect("the selection trimmed");
        assert_eq!(selection::plain_text(&found.word, &doc), " quick brown ");
        assert_eq!(
            found.matches.len(),
            3,
            "the spaces at the ends do not count"
        );
    }

    #[test]
    fn a_selection_from_inside_a_word_lights_the_same_fragment_elsewhere() {
        let doc = markdown::parse("Aggregates samples into fixed-width buckets.\n\nAggregates samples into fixed-width buckets.");
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 21,
            },
            end: ModelPos {
                block: 0,
                span: 0,
                byte: 35,
            },
        };
        assert_eq!(selection::plain_text(&sel, &doc), "to fixed-width");
        let found = find(&doc, sel, Match::Text).expect("the other paragraph");
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].start.block, 1);
    }

    #[test]
    fn a_triple_clicked_line_of_code_lights_its_identical_lines() {
        let doc = load::code_document(
            Some("rust"),
            "/// Aggregates samples into buckets.\npub struct A;\n\n/// Aggregates samples into buckets.\npub struct B;\n",
        );
        let line = selection::paragraph_at(
            &doc,
            ModelPos {
                block: 0,
                span: 0,
                byte: 4,
            },
        )
        .expect("the line under the triple click");
        assert_eq!(
            selection::plain_text(&line, &doc).trim(),
            "/// Aggregates samples into buckets."
        );
        let found = find(&doc, line, Match::Text).expect("the identical line");
        assert_eq!(found.matches.len(), 1);
        assert!(!found.matches.contains(&line));
    }

    #[test]
    fn a_selection_across_paragraphs_lights_nothing() {
        let doc = markdown::parse("one two\n\none two");
        let sel = Selection {
            start: ModelPos {
                block: 0,
                span: 0,
                byte: 4,
            },
            end: ModelPos {
                block: 1,
                span: 0,
                byte: 3,
            },
        };
        assert!(selection::plain_text(&sel, &doc).contains('\n'));
        assert!(find(&doc, sel, Match::Text).is_none());
    }

    #[test]
    fn a_word_that_stands_alone_lights_nothing() {
        let doc = markdown::parse("The panel opens.");
        assert!(find(&doc, word(&doc, 0, 0, 5), Match::Word).is_none());
    }

    #[test]
    fn a_word_styled_in_two_spans_is_left_out_whole() {
        let doc = markdown::parse("A **pan**el here, a panel there.");
        let clicked = word(&doc, 0, 1, 1);
        assert_eq!(selection::plain_text(&clicked, &doc), "panel");
        let found = find(&doc, clicked, Match::Word).expect("the plain one");
        assert_eq!(found.matches.len(), 1);
        assert!(found.matches[0].start > clicked.end);
    }

    #[test]
    fn a_code_file_lights_whole_identifiers_only() {
        let doc = load::code_document(
            Some("rust"),
            "let count = 1;\nlet recount = count + count_all;\ncount\n",
        );
        let clicked = word(&doc, 0, 0, 5);
        assert_eq!(selection::plain_text(&clicked, &doc), "count");
        let found = find(&doc, clicked, Match::Word).expect("two others");
        assert_eq!(found.matches.len(), 2, "not recount, not count_all");
    }
}

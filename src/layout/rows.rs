//! The rows of a code line counted without shaping it.
//!
//! The measure of a big code block shapes each line to learn its height
//! and drops the result for all but the lines near the view. For a line
//! of plain ASCII in a face that neither kerns nor joins its characters,
//! the height follows from the widths of the characters and the wrap
//! width, by the rule the shaper applies (cosmic-text's
//! `ShapeLine::layout_to_buffer` with `Wrap::WordOrGlyph`): words end at
//! the break opportunities of UAX 14, each trailing space is a word of
//! its own that the end of a row may swallow, and a word wider than the
//! row is cut character by character. The sums are added in the shaper's
//! order, so the height is the shaper's to the bit.

use crate::style::fonts::FontStore;
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The first and the last byte a counted line may hold: printable
/// ASCII. A tab moves to a stop and a control byte is replaced before
/// the shaper sees it, so such a line is shaped.
const FIRST: u8 = 0x20;
const LAST: u8 = 0x7e;

/// The advances of the printable ASCII characters of one face at one
/// size, as the shaper gives them for a character alone.
pub(crate) struct Widths {
    size_bits: u32,
    advance: [f32; 128],
    /// The widest of the advances.
    widest: f32,
}

impl Widths {
    /// The table for `face` at `size`, or None when the face cannot be
    /// counted: a pair of characters that is not laid out as its two
    /// characters alone (kerning, a ligature) would make the sums wrong.
    pub(crate) fn of(fonts: &mut FontStore, face: &str, size: f32) -> Option<Widths> {
        if !face_counts(fonts, face) {
            return None;
        }
        let mut advance = [0.0_f32; 128];
        let mut text = [0u8; 1];
        for byte in FIRST..=LAST {
            text[0] = byte;
            let glyphs = shaped_glyphs(fonts, face, size, std::str::from_utf8(&text).ok()?);
            let [glyph] = glyphs[..] else {
                return None;
            };
            advance[byte as usize] = glyph.width;
        }
        Some(Widths {
            size_bits: size.to_bits(),
            advance,
            widest: advance.iter().copied().fold(0.0, f32::max),
        })
    }

    pub(crate) fn is_for(&self, size: f32) -> bool {
        self.size_bits == size.to_bits()
    }

    /// The height the shaper gives `line` wrapped at `wrap`: a row's
    /// top is the line height added once per row above, as the shaper's
    /// run iterator adds it.
    pub(crate) fn height(&self, line: &str, wrap: f32, line_height: f32) -> f32 {
        let mut height = 0.0_f32;
        for _ in 0..self.rows(line, wrap) {
            height += line_height;
        }
        height.max(line_height)
    }

    /// The rows `line` takes at `wrap`. The line is `countable`.
    ///
    /// A row is counted when the shaper closes it with something on it.
    /// The shaper closes a row from where it began to where the next one
    /// begins, and leaves out a row whose two ends are the same place: a
    /// character wider than the row closes the empty row before it, and
    /// a word that does not fit closes the row without the space before
    /// it, which may be all the row held.
    pub(crate) fn rows(&self, line: &str, wrap: f32) -> usize {
        let bytes = line.as_bytes();
        // A line that fits with a pixel to spare is one row whatever its
        // words: each sum below stays under the row's width. The pixel
        // is far more than the rounding of the sums of a line this short.
        if bytes.len() as f32 * self.widest <= wrap - 1.0 {
            return 1;
        }
        let mut rows = 0usize;
        // The width of the words the open row holds, and where it starts.
        let mut range = 0.0_f32;
        let mut start = 0usize;
        // Whether the word placed last was a space.
        let mut after_blank = false;
        let mut place = |from: usize, to: usize, blank: bool| {
            let mut close = |start: &mut usize, end: usize, next: usize| {
                if end != *start {
                    rows += 1;
                }
                *start = next;
            };
            let mut width = 0.0_f32;
            for &b in &bytes[from..to] {
                width += self.advance[b as usize];
            }
            if range + width <= wrap || (blank && range <= wrap) {
                range += width;
            } else if width > wrap {
                if range > 0.0 {
                    close(&mut start, from, from);
                    range = 0.0;
                }
                for (i, &b) in bytes[from..to].iter().enumerate() {
                    let glyph = self.advance[b as usize];
                    if range + glyph <= wrap {
                        range += glyph;
                    } else {
                        close(&mut start, from + i, from + i);
                        range = glyph;
                    }
                }
            } else {
                // A space is one byte, so the space before this word
                // starts at `from - 1`.
                let end = if after_blank { from - 1 } else { from };
                let next = if blank { to } else { from };
                if range > 0.0 {
                    close(&mut start, end, next);
                } else {
                    start = next;
                }
                range = if blank { 0.0 } else { width };
            }
            after_blank = blank;
        };
        let mut word = 0usize;
        for (end, _) in unicode_linebreak::linebreaks(line) {
            let mut spaces = end;
            while spaces > word && bytes[spaces - 1] == b' ' {
                spaces -= 1;
            }
            if word < spaces {
                place(word, spaces, false);
            }
            for at in spaces..end {
                place(at, at + 1, true);
            }
            word = end;
        }
        // The last row, unless a swallowed space was all that was left.
        if start < bytes.len() || rows == 0 {
            rows += 1;
        }
        rows
    }
}

/// Whether `line` can be counted: printable ASCII within the length the
/// shaper takes in one piece.
pub(crate) fn countable(line: &str) -> bool {
    line.len() <= crate::style::highlight::LONG_LINE
        && line.bytes().all(|b| (FIRST..=LAST).contains(&b))
}

/// A glyph of a shaped text: its advance, and its number in its font.
#[derive(Clone, Copy)]
struct Glyph {
    width: f32,
    id: u16,
    font: cosmic_text::fontdb::ID,
    weight: cosmic_text::fontdb::Weight,
}

/// The glyphs of `text` shaped alone, as a code line is.
fn shaped_glyphs(fonts: &mut FontStore, face: &str, size: f32, text: &str) -> Vec<Glyph> {
    let mut buffer = Buffer::new(&mut fonts.font_system, Metrics::new(size, size));
    buffer.set_size(&mut fonts.font_system, None, None);
    let attrs = Attrs::new().family(Family::Name(face));
    buffer.set_rich_text(
        &mut fonts.font_system,
        [(text, attrs.clone())],
        &attrs,
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.font_system, false);
    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
        glyphs.extend(run.glyphs.iter().map(|g| Glyph {
            width: g.w,
            id: g.glyph_id,
            font: g.font_id,
            weight: g.font_weight,
        }));
    }
    glyphs
}

/// Whether each pair of printable ASCII characters of `face` is laid
/// out as the two characters alone: the same advances, and the glyphs
/// the font's character map gives. The shaper allows no line break
/// inside a pair of punctuation marks whose glyphs the font replaces
/// (`->` in a font with programming ligatures), so the breaks of such a
/// face are not those of UAX 14. Read once per face: an advance scales
/// with the size, so the answer holds at each size.
fn face_counts(fonts: &mut FontStore, face: &str) -> bool {
    static FACES: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let faces = FACES.get_or_init(Default::default);
    if let Some(&known) = faces
        .lock()
        .ok()
        .and_then(|f| f.get(face).copied())
        .as_ref()
    {
        return known;
    }
    const SIZE: f32 = 20.0;
    let mut alone: [Option<Glyph>; 128] = [None; 128];
    let mut text = [0u8; 2];
    let mut counts = true;
    for byte in FIRST..=LAST {
        text[0] = byte;
        let glyphs = shaped_glyphs(
            fonts,
            face,
            SIZE,
            std::str::from_utf8(&text[..1]).unwrap_or(""),
        );
        let mapped = match glyphs[..] {
            [glyph] => fonts
                .font_system
                .get_font(glyph.font, glyph.weight)
                .is_some_and(|font| font.as_swash().charmap().map(char::from(byte)) == glyph.id)
                .then_some(glyph),
            _ => None,
        };
        match mapped {
            Some(glyph) => alone[byte as usize] = Some(glyph),
            None => counts = false,
        }
    }
    'pairs: for first in FIRST..=LAST {
        if !counts {
            break;
        }
        for second in FIRST..=LAST {
            // A space is a word of its own for the shaper: it meets no
            // neighbour.
            if first == b' ' || second == b' ' {
                continue;
            }
            text = [first, second];
            let glyphs = shaped_glyphs(fonts, face, SIZE, std::str::from_utf8(&text).unwrap_or(""));
            let same = |glyph: Glyph, byte: u8| {
                alone[byte as usize].is_some_and(|a| {
                    a.width.to_bits() == glyph.width.to_bits()
                        && a.id == glyph.id
                        && a.font == glyph.font
                })
            };
            if !matches!(glyphs[..], [a, b] if same(a, first) && same(b, second)) {
                counts = false;
                break 'pairs;
            }
        }
    }
    if let Ok(mut faces) = faces.lock() {
        faces.insert(face.to_string(), counts);
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::engine::shaped_code_line_height;
    use crate::style::fonts::{BODY_FAMILY, CODE_FAMILY};

    fn check(fonts: &mut FontStore, widths: &Widths, line: &str, size: f32, wrap: f32) {
        let line_height = crate::layout::metrics::LINE_HEIGHT * size;
        let shaped = shaped_code_line_height(fonts, line, size, line_height, wrap);
        let counted = widths.height(line, wrap, line_height);
        assert_eq!(
            counted.to_bits(),
            shaped.to_bits(),
            "size {size} wrap {wrap}: counted {counted}, shaped {shaped}, line {line:?}"
        );
    }

    #[test]
    fn the_code_face_counts_and_the_body_face_does_not() {
        let mut fonts = FontStore::new();
        assert!(Widths::of(&mut fonts, CODE_FAMILY, 20.0).is_some());
        assert!(Widths::of(&mut fonts, BODY_FAMILY, 22.0).is_none());
    }

    #[test]
    fn a_counted_height_is_the_shaped_height_to_the_bit() {
        let mut fonts = FontStore::new();
        let mut lines: Vec<String> = vec![
            "x".into(),
            " ".into(),
            "        ".into(),
            "a b".into(),
            "trailing spaces      ".into(),
            "   leading and  double  spaces  inside   ".into(),
            "#EXTINF:-1 tvg-id=\"\" tvg-name=\"FR - A long name (2020)\" tvg-logo=\"https://example.org/t/p/w600_and_h900_bestv2/abcdefghijklmnopqrstuvwxyz.jpg\" group-title=\"SRS | FRENCH\",FR - A long name S01 E06".into(),
            "http://example.org:80/series/F745914C9554/F9A32618522D/888852.mkv".into(),
            "a".repeat(700),
            "word ".repeat(300),
            "a-b/c".repeat(200),
            "!=|>->==::<>".repeat(60),
            format!("{}{}", " ".repeat(90), "x".repeat(90)),
            format!("{} {}", "y".repeat(69), "z".repeat(3)),
            format!("{}  {}", "y".repeat(68), "z".repeat(3)),
            "fn main() { let value = some_function(argument_one, argument_two).unwrap_or_default(); }".into(),
        ];
        let source = std::fs::read_to_string("tests/fixtures/huge.rs").unwrap();
        lines.extend(source.lines().take(4000).step_by(3).map(str::to_string));
        for (size, wraps) in [
            (20.0_f32, [829.0_f32, 333.5, 40.0]),
            (22.0, [829.0, 500.0, 61.3]),
        ] {
            let widths = Widths::of(&mut fonts, CODE_FAMILY, size).unwrap();
            assert!(widths.is_for(size));
            for wrap in wraps {
                for line in lines.iter().filter(|l| !l.is_empty()) {
                    assert!(countable(line), "{line:?}");
                    check(&mut fonts, &widths, line, size, wrap);
                }
            }
        }
    }

    #[test]
    fn a_row_the_shaper_leaves_empty_is_not_counted() {
        let mut fonts = FontStore::new();
        // A character wider than the row: the shaper opens no row before it.
        for (size, wrap) in [(96.0_f32, 50.0_f32), (69.0, 40.0), (120.0, 58.0)] {
            let widths = Widths::of(&mut fonts, CODE_FAMILY, size).unwrap();
            for line in ["a", "ab", "abc def", "x y", "fn main() {}", "   ", "a  b"] {
                check(&mut fonts, &widths, line, size, wrap);
            }
        }
        // One space, then a word that fills the row: the shaper drops the
        // space and the row it was alone on.
        let (size, wrap) = (20.0_f32, 829.0_f32);
        let widths = Widths::of(&mut fonts, CODE_FAMILY, size).unwrap();
        let fill = (wrap / widths.advance[b'a' as usize]).floor() as usize;
        for line in [
            format!(" {}", "a".repeat(fill)),
            format!(" {}", "a".repeat(fill - 1)),
            format!("  {}", "a".repeat(fill)),
            format!("x {}", "a".repeat(fill)),
            format!("{} {}", "b".repeat(fill), "a".repeat(fill)),
            format!("{}   {}", "b".repeat(fill - 1), "a".repeat(fill)),
        ] {
            check(&mut fonts, &widths, &line, size, wrap);
        }
        for (size, wrap, line) in [
            (32.5_f32, 50.0_f32, " r;"),
            (22.0, 50.0, " = /"),
            (14.4, 50.0, "i<&X y  |#   iMf'y"),
        ] {
            let widths = Widths::of(&mut fonts, CODE_FAMILY, size).unwrap();
            check(&mut fonts, &widths, line, size, wrap);
        }
    }

    /// The count against the shaper: random lines of printable ASCII,
    /// one character in three a space, at sizes up to a character wider
    /// than the row.
    #[test]
    #[ignore]
    fn fuzz_rows() {
        use proptest::prelude::*;
        let config = ProptestConfig {
            failure_persistence: None,
            ..ProptestConfig::default()
        };
        let fonts = std::cell::RefCell::new(FontStore::new());
        let character = prop_oneof![
            1 => Just(' '),
            2 => (0x21u8..=0x7e).prop_map(char::from),
        ];
        proptest!(config, |(
            line in prop::collection::vec(character, 1..120),
            size in prop::sample::select(vec![14.4_f32, 16.94, 20.0, 22.0, 26.4, 32.5, 69.0, 96.0]),
            wrap in 40.0_f32..900.0,
        )| {
            let line: String = line.into_iter().collect();
            let fonts = &mut *fonts.borrow_mut();
            let widths = Widths::of(fonts, CODE_FAMILY, size).unwrap();
            let line_height = crate::layout::metrics::LINE_HEIGHT * size;
            let shaped = shaped_code_line_height(fonts, &line, size, line_height, wrap);
            let counted = widths.height(&line, wrap, line_height);
            prop_assert_eq!(
                counted.to_bits(),
                shaped.to_bits(),
                "size {} wrap {}: counted {}, shaped {}, line {:?}",
                size, wrap, counted, shaped, line
            );
        });
    }

    #[test]
    fn a_tab_a_control_byte_and_a_letter_outside_ascii_are_not_counted() {
        assert!(!countable("a\tb"));
        assert!(!countable("a\u{1}b"));
        assert!(!countable("caf\u{e9}"));
        assert!(!countable(
            &"x".repeat(crate::style::highlight::LONG_LINE + 1)
        ));
        assert!(countable(&"x".repeat(crate::style::highlight::LONG_LINE)));
    }
}

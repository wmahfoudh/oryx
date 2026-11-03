//! Go to line: what the field's text means, where a line and a column
//! stand in the source, the `file:line:column` form of the command
//! line, and the pill the field is drawn in. The numbers are the ones
//! the line numbers show and compilers print: lines and columns count
//! from 1, a column counts characters.

use std::path::Path;

use crate::paint::painter::Painter;
use crate::style::fonts::CODE_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::search::{self, FieldView};
use crate::ui::textfield::TextField;

const BAR_WIDTH: f32 = 240.0;

/// A place asked for: a line, and a column on it when one was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub line: usize,
    pub column: Option<usize>,
}

/// Reads `412` or `412:10`. A colon with nothing after it is a line
/// still being typed, and zero reads as 1, the way an editor clamps.
/// Anything else is no target.
pub fn parse(text: &str) -> Option<Target> {
    let text = text.trim();
    let (line, column) = match text.split_once(':') {
        Some((line, column)) => (line, Some(column)),
        None => (text, None),
    };
    let line = number(line)?;
    let column = match column {
        Some("") | None => None,
        Some(column) => Some(number(column)?),
    };
    Some(Target { line, column })
}

fn number(digits: &str) -> Option<usize> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // A number too long for the type is past the end of any file.
    Some(digits.parse().unwrap_or(usize::MAX).max(1))
}

/// Whether the field takes `text` as typed or pasted: digits and the
/// colon, nothing else, so the field never holds what `parse` refuses
/// for its characters.
pub fn accepts(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b':')
}

/// The number of lines in `source`, the row after a final line break
/// counted, as the line numbers count it.
pub fn line_count(source: &str) -> usize {
    source.bytes().filter(|&b| b == b'\n').count() + 1
}

/// The source offset of a target. A line past the end is the last
/// line, and a column past the end of its line is the line's end.
pub fn offset(source: &str, target: Target) -> usize {
    let mut start = 0;
    for _ in 1..target.line {
        match source[start..].find('\n') {
            Some(at) => start += at + 1,
            None => break,
        }
    }
    let line = source[start..].split('\n').next().unwrap_or("");
    let column = target.column.unwrap_or(1);
    let within = line
        .char_indices()
        .nth(column - 1)
        .map_or(line.len(), |(at, _)| at);
    start + within
}

/// Splits a command line argument of the form `file:412` or
/// `file:412:10`. The name as typed wins when such a file exists, so a
/// file really called `notes:412` still opens; the numbers are read
/// only when it does not.
pub fn split_arg(arg: &Path, exists: impl Fn(&Path) -> bool) -> (&Path, Option<Target>) {
    if exists(arg) {
        return (arg, None);
    }
    let Some(text) = arg.to_str() else {
        return (arg, None);
    };
    // The readings with one number and with two, each a name and what
    // follows it; an empty name is no reading.
    let one = text.rfind(':').filter(|&cut| cut > 0);
    let two = one.and_then(|cut| text[..cut].rfind(':').filter(|&cut| cut > 0));
    let reading =
        |cut: usize| parse(&text[cut + 1..]).map(|target| (Path::new(&text[..cut]), Some(target)));
    let (one, two) = (one.and_then(reading), two.and_then(reading));
    // A name that exists wins, the longer one first; with no file to
    // tell, two numbers are what a compiler prints.
    [one, two]
        .into_iter()
        .flatten()
        .find(|(path, _)| exists(path))
        .or(two)
        .or(one)
        .unwrap_or((arg, None))
}

/// The open field: its text, where it was last drawn, and the last
/// line of the file, shown beside it.
pub struct GotoState {
    pub field: TextField,
    pub view: FieldView,
    pub lines: usize,
}

impl GotoState {
    pub fn new(lines: usize) -> Self {
        GotoState {
            field: TextField::new(String::new()),
            view: FieldView::default(),
            lines,
        }
    }

    /// The target the field holds now.
    pub fn target(&self) -> Option<Target> {
        parse(self.field.text())
    }
}

/// The pill's box for a window `width` wide: x, y, width, height.
pub fn bar_rect(width: f32) -> (f32, f32, f32, f32) {
    let x = (width - BAR_WIDTH - search::MARGIN).max(search::MARGIN);
    (x, search::MARGIN, BAR_WIDTH, search::BAR_HEIGHT)
}

/// Whether a point at logical window coordinates lies on the pill.
pub fn bar_hit(width: f32, px: f32, py: f32) -> bool {
    let (x, y, w, h) = bar_rect(width);
    px >= x && px < x + w && py >= y && py < y + h
}

/// The floating pill over the document's top-right corner, where the
/// search bar stands: the field on the left, the file's line count on
/// the right. `caret_lit` is the blink's lit half while the field has
/// the keys.
pub fn draw_bar(
    painter: &mut Painter,
    theme: &Theme,
    state: &mut GotoState,
    width: f32,
    caret_lit: bool,
) {
    let ui = &theme.ui;
    let (x, y, w, h) = bar_rect(width);
    for (grow, alpha) in [(6.0, 16), (3.0, 28)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            w + 2.0 * grow,
            h + 2.0 * grow,
            search::RADIUS + grow,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: alpha,
            },
        );
    }
    painter.fill(x, y, w, h, search::RADIUS, ui.overlay_bg);
    painter.stroke(x, y, w, h, search::RADIUS, 1.0, theme.blocks.table_border);

    let count = format!("of {}", state.lines);
    let count_w = painter.measure(&count, CODE_FAMILY, search::COUNTER_SIZE, 400);
    painter.text(
        x + w - search::PAD - count_w,
        y + 10.0 + search::TEXT_DROP + (search::QUERY_SIZE - search::COUNTER_SIZE) * 1.1,
        &count,
        CODE_FAMILY,
        search::COUNTER_SIZE,
        400,
        theme.blocks.frontmatter_fg,
    );
    let avail = w - 2.0 * search::PAD - count_w - 12.0;
    state.view = search::draw_field(
        painter,
        theme,
        &state.field,
        x + search::PAD,
        y + 10.0,
        avail,
        "line:column",
        true,
        caret_lit,
        state.view.window.start,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, column: Option<usize>) -> Option<Target> {
        Some(Target { line, column })
    }

    #[test]
    fn the_field_reads_a_line_and_an_optional_column() {
        assert_eq!(parse("412"), at(412, None));
        assert_eq!(parse("412:10"), at(412, Some(10)));
        assert_eq!(parse(" 7 "), at(7, None));
        assert_eq!(parse("412:"), at(412, None), "the column is still to come");
        assert_eq!(parse("0"), at(1, None), "zero is the first line");
        assert_eq!(parse("3:0"), at(3, Some(1)), "and the first column");
        assert_eq!(
            parse("99999999999999999999999"),
            at(usize::MAX, None),
            "too long for the type is past any end"
        );
        for refused in ["", ":", ":4", "4:5:6", "12a", "-3", "4 :5", "1.5"] {
            assert_eq!(parse(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn the_field_takes_digits_and_the_colon_only() {
        for taken in ["4", "412", ":", "41:2"] {
            assert!(accepts(taken), "{taken:?}");
        }
        for refused in ["", "a", "4a", " ", "-", "4.2", "\n", "main.rs:4"] {
            assert!(!accepts(refused), "{refused:?}");
        }
    }

    #[test]
    fn a_target_lands_on_its_line_and_column() {
        let source = "one\ntwo words\n\nfour\n";
        let go = |line, column| offset(source, Target { line, column });
        assert_eq!(go(1, None), 0);
        assert_eq!(go(2, None), 4);
        assert_eq!(go(2, Some(1)), 4);
        assert_eq!(go(2, Some(5)), 8);
        assert_eq!(go(3, None), 14, "an empty line");
        assert_eq!(go(3, Some(9)), 14, "has only its start");
        assert_eq!(go(4, None), 15);
        assert_eq!(go(5, None), 20, "the row after the final line break");
        assert_eq!(line_count(source), 5);
    }

    #[test]
    fn a_target_past_the_end_lands_on_the_last_line_or_the_line_end() {
        let source = "one\ntwo";
        let go = |line, column| offset(source, Target { line, column });
        assert_eq!(go(9, None), 4, "the last line's start");
        assert_eq!(go(usize::MAX, Some(2)), 5, "its column still counts");
        assert_eq!(go(1, Some(99)), 3, "the end of the line, before the break");
        assert_eq!(go(2, Some(99)), 7);
        assert_eq!(
            offset(
                "",
                Target {
                    line: 3,
                    column: Some(3)
                }
            ),
            0
        );
        assert_eq!(line_count(""), 1);
    }

    #[test]
    fn a_column_counts_characters() {
        let source = "é\tü-x\n";
        let go = |column| {
            offset(
                source,
                Target {
                    line: 1,
                    column: Some(column),
                },
            )
        };
        assert_eq!(go(2), 2, "after the two bytes of é");
        assert_eq!(go(3), 3, "a tab is one character");
        assert_eq!(go(4), 5);
        assert!(source.is_char_boundary(go(4)));
    }

    #[test]
    fn the_command_line_form_splits_the_numbers_from_the_name() {
        let none = |_: &Path| false;
        let split = |arg: &'static str| {
            let (path, target) = split_arg(Path::new(arg), none);
            (path.to_str().unwrap(), target)
        };
        assert_eq!(split("main.rs:412"), ("main.rs", at(412, None)));
        assert_eq!(
            split("src/main.rs:412:10"),
            ("src/main.rs", at(412, Some(10)))
        );
        assert_eq!(split("main.rs:412:"), ("main.rs", at(412, None)));
        assert_eq!(split("main.rs"), ("main.rs", None));
        assert_eq!(split("main.rs:abc"), ("main.rs:abc", None));
        assert_eq!(split(":412"), (":412", None), "no name, no split");
        assert_eq!(
            split("C:\\notes\\a.md:7"),
            ("C:\\notes\\a.md", at(7, None)),
            "a drive letter's colon is not a number"
        );
        assert_eq!(split("C:\\notes\\a.md"), ("C:\\notes\\a.md", None));
    }

    #[test]
    fn a_file_really_named_with_numbers_opens_as_typed() {
        let exists = |path: &Path| path == Path::new("notes:412");
        let (path, target) = split_arg(Path::new("notes:412"), exists);
        assert_eq!(path, Path::new("notes:412"));
        assert_eq!(target, None);
        let (path, target) = split_arg(Path::new("notes:412:3"), exists);
        assert_eq!(path, Path::new("notes:412"), "the longest real name wins");
        assert_eq!(target, at(3, None));
    }
}

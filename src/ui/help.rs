//! The help page: markdown generated in memory and shown as a document
//! of Oryx's own, so help is searchable, themed and scrollable. The
//! shortcut tables are built from `keymap::SHORTCUTS`, the dispatch
//! truth, and only the surrounding prose is authored, so the page can
//! never drift from what the keys do. Nothing lands on disk, and with
//! no file behind it the page cannot be edited, saved or reloaded.

use crate::input::keymap;

/// The generated page.
pub fn page() -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(4096);
    let _ = write!(out, "# Oryx v{} Shortcuts\n\n", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "Press {} or {} to close this. \
         Please refer to the full documentation in the project README, on \
         [Codeberg](https://codeberg.org/wmahfoudh/oryx) or \
         [GitHub](https://github.com/wmahfoudh/oryx).",
        keymap::display("F1"),
        keymap::display("Escape"),
    );
    let mut section = "";
    for row in keymap::SHORTCUTS {
        if row.section != section {
            section = row.section;
            let _ = write!(out, "\n## {section}\n\n| Shortcut | Action |\n|---|---|\n");
        }
        let _ = writeln!(out, "| `{}` | {} |", keymap::display(row.keys), row.action);
    }
    out.push_str("\n## While editing\n\n");
    let _ = writeln!(
        out,
        "The arrows, `Home`, `End`, `Page Up` and `Page Down` move the caret. \
         `{}` / `{}` jump by word, `{}` / `{}` jump to the ends of the file, and \
         `{}` / `{}` delete by word. Typing replaces a selection, and `{}` followed \
         by typing replaces the whole file.",
        keymap::display("Ctrl+Left"),
        keymap::display("Ctrl+Right"),
        keymap::display("Ctrl+Home"),
        keymap::display("Ctrl+End"),
        keymap::display("Ctrl+Backspace"),
        keymap::display("Ctrl+Delete"),
        keymap::display("Ctrl+A"),
    );
    out.push_str(
        "\nClosing, quitting or reloading with unsaved changes asks first: \
         `Enter` saves, `D` discards, `Escape` keeps editing.\n",
    );
    out.push_str("\n## Mouse and touch\n\n");
    out.push_str(
        "A double click selects the word, a triple click the paragraph or the code line. \
         The wheel scrolls, and dragging the scrollbar or its track jumps. On a touch \
         screen, swiping scrolls with momentum, tapping clicks, and a two-finger pinch \
         zooms.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_stands_on_the_page() {
        let page = page();
        for row in keymap::SHORTCUTS {
            assert!(
                page.contains(&keymap::display(row.keys)),
                "the page names {}",
                row.keys
            );
            assert!(
                page.contains(row.action),
                "the page describes {}",
                row.action
            );
        }
        for section in ["Files", "Navigation", "Edit", "Export"] {
            assert!(page.contains(section), "the page has a {section} caption");
        }
    }

    #[test]
    fn the_page_names_the_version_and_the_project_homes() {
        let page = page();
        assert!(
            page.contains(env!("CARGO_PKG_VERSION")),
            "the running version stands on the page"
        );
        assert!(page.contains("https://codeberg.org/wmahfoudh/oryx"));
        assert!(page.contains("https://github.com/wmahfoudh/oryx"));
    }
}

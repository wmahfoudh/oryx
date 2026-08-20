//! The help page: markdown generated in memory and shown as a document
//! of Oryx's own, so help is searchable, themed and scrollable. The
//! shortcut tables are built from `keymap::SHORTCUTS`, the dispatch
//! truth, and only the surrounding prose is authored, so the page can
//! never drift from what the keys do. Nothing lands on disk, and with
//! no file behind it the page cannot be edited, saved or reloaded.

use crate::input::keymap;

/// The page an empty launch shows in the document area: how to open a
/// file, where the folder sidebar is, and where the shortcuts are. No
/// file stands behind it, so it cannot be edited or saved, and the
/// first file opened replaces it.
pub fn welcome() -> String {
    format!(
        "# Oryx\n\n\
         Press `{}` to open a file.\n\n\
         `{}` shows the folder sidebar, to browse and open files from there.\n\n\
         `{}` lists the shortcuts.\n\n\
         A file dropped onto this window opens too.\n",
        keymap::display("Ctrl+O"),
        keymap::display("Ctrl+Shift+B"),
        keymap::display("F1"),
    )
}

/// The generated page.
pub fn page() -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(4096);
    let _ = write!(out, "# Oryx v{} Shortcuts\n\n", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "Press {} or {} to close this. \
         Please refer to the full documentation on \
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
         by typing replaces the whole file. `Enter` keeps the line's indentation, \
         and in a markdown file it continues lists and quotes. `Tab` indents and \
         `Shift+Tab` removes an indent, over every selected line at once; at a \
         list marker, `Tab` nests the item.",
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
    let _ = writeln!(
        out,
        "A double click selects the word, a triple click the paragraph or the code line. \
         The wheel scrolls, and with `{}` held it zooms; dragging the scrollbar or its \
         track jumps. A file dropped onto the window opens, and a dropped folder opens \
         the sidebar on it. On a touch screen, swiping scrolls with momentum, tapping \
         clicks, and a two-finger pinch zooms.",
        keymap::display("Ctrl"),
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
    fn the_welcome_page_names_the_three_chords_and_nothing_unbound() {
        let page = welcome();
        for keys in ["Ctrl+O", "Ctrl+Shift+B", "F1"] {
            assert!(
                page.contains(&format!("`{}`", keymap::display(keys))),
                "the welcome page names {keys}"
            );
        }
        let bound: Vec<String> = keymap::SHORTCUTS
            .iter()
            .map(|row| keymap::display(row.keys))
            .collect();
        for chord in page.split('`').skip(1).step_by(2) {
            assert!(
                bound.contains(&chord.to_string()),
                "the welcome page names {chord}, which the keymap does not bind"
            );
        }
        assert!(page.starts_with("# Oryx\n"), "the page opens on the name");
        assert!(page.lines().count() <= 10, "the page stays short: {page}");
    }

    #[test]
    fn the_mouse_paragraph_names_dropping_and_the_wheel_zoom() {
        let page = page();
        assert!(page.contains("dropped onto the window opens"));
        assert!(page.contains(&format!("with `{}` held it zooms", keymap::display("Ctrl"))));
        assert!(welcome().contains("dropped onto this window opens"));
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

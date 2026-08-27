//! The help page: markdown generated in memory and shown as a document
//! of Oryx's own, so help is searchable, themed and scrollable. The
//! shortcut tables are built from `keymap::SHORTCUTS`, the dispatch
//! truth, and only the surrounding prose is authored, so the page can
//! never drift from what the keys do. Nothing lands on disk, and with
//! no file behind it the page cannot be edited, saved or reloaded.

use crate::input::keymap;

/// The page an empty launch shows in the document area: how to open a
/// file, where the folder sidebar is, where the settings are (a fresh
/// install on a scaled screen starts there), where the shortcuts are,
/// and where the documentation is. No file stands behind it, so it
/// cannot be edited or saved, and the first file opened replaces it.
pub fn welcome() -> String {
    format!(
        "# Oryx\n\n\
         Press `{}` to open a file.\n\n\
         `{}` shows the folder sidebar, to browse and open files from there.\n\n\
         `{}` opens the settings: fonts, sizes and the interface scale, \
         if the page looks too small or too large on this screen.\n\n\
         `{}` lists the shortcuts.\n\n\
         You can also drag and drop a file here.\n\n\
         Please refer to the full documentation on \
         [GitHub](https://github.com/wmahfoudh/oryx).\n",
        keymap::display("Ctrl+O"),
        keymap::display("Ctrl+Shift+B"),
        keymap::display("Ctrl+,"),
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
    out.push_str("\n## Sidebar\n\n");
    let _ = writeln!(
        out,
        "`{}` moves the keys to the sidebar and `{}` brings them back to the document. \
         In the sidebar, `Up` and `Down` move the selection, `Enter` opens the selected \
         file or folder (the `..` row goes up), or jumps to the selected heading on the \
         outline tab, and `{}` switches between the files and the outline.",
        keymap::display("Left"),
        keymap::display("Right"),
        keymap::display("Ctrl+Tab"),
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

    /// The settings dialog sets the fonts, the sizes and the interface
    /// scale; the row that opens it says so, since a fresh install on a
    /// scaled screen starts there.
    #[test]
    fn the_settings_row_names_the_interface_scale() {
        let row = keymap::SHORTCUTS
            .iter()
            .find(|row| row.keys == "Ctrl+,")
            .expect("the settings row");
        assert!(row.action.contains("scale"), "{}", row.action);
    }

    #[test]
    fn the_arrow_row_says_where_the_keys_go() {
        let row = keymap::SHORTCUTS
            .iter()
            .find(|row| row.keys == "Left / Right")
            .expect("the arrow row");
        assert_eq!(row.action, "Move to the sidebar / to the document");
    }

    #[test]
    fn the_sidebar_paragraph_says_what_enter_does_there() {
        let page = page();
        assert!(page.contains("## Sidebar"), "{page}");
        assert!(page.contains("opens the selected file or folder"), "{page}");
        assert!(page.contains("outline"), "{page}");
    }

    #[test]
    fn the_welcome_page_points_at_the_settings_and_the_documentation() {
        let page = welcome();
        assert!(
            page.contains(&format!(
                "`{}` opens the settings",
                keymap::display("Ctrl+,")
            )),
            "{page}"
        );
        assert!(page.contains("interface scale"), "{page}");
        assert!(
            page.contains("[GitHub](https://github.com/wmahfoudh/oryx)"),
            "{page}"
        );
    }

    #[test]
    fn the_welcome_page_names_the_four_chords_and_nothing_unbound() {
        let page = welcome();
        for keys in ["Ctrl+O", "Ctrl+Shift+B", "Ctrl+,", "F1"] {
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
        assert!(page.lines().count() <= 14, "the page stays short: {page}");
    }

    #[test]
    fn the_mouse_paragraph_names_dropping_and_the_wheel_zoom() {
        let page = page();
        assert!(page.contains("dropped onto the window opens"));
        assert!(page.contains(&format!("with `{}` held it zooms", keymap::display("Ctrl"))));
        assert!(welcome().contains("drag and drop a file here"));
    }

    #[test]
    fn the_page_names_the_version_and_the_project_homes() {
        let page = page();
        assert!(
            page.contains(env!("CARGO_PKG_VERSION")),
            "the running version stands on the page"
        );
        assert!(page.contains("https://github.com/wmahfoudh/oryx"));
        assert!(!page.contains("codeberg"), "GitHub is the only host named");
    }
}

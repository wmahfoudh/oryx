use std::time::Duration;

pub mod keymap;
pub mod touch;

/// Two clicks closer together than this count as one double click, wherever
/// the pointer is: a list row, a text field, or the sidebar edge.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(450);

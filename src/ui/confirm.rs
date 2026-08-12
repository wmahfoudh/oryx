//! The unsaved-changes confirm: a minimal modal guarding close, quit,
//! and manual reload while edits are unsaved. Enter saves and
//! proceeds, D discards and proceeds, Escape cancels; every other key
//! is consumed and ignored, since a modal owns the keyboard.

use winit::keyboard::{Key, NamedKey};

/// What the guarded action was, carried by the overlay until a key
/// decides. An open remembers whether it reroots the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Quit,
    Reload,
    Open(std::path::PathBuf, bool),
    New,
}

/// The user's decision on the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Save the edits, then run the pending action.
    Save,
    /// Drop the edits, then run the pending action.
    Discard,
    /// Keep editing; the pending action dies.
    Cancel,
    /// The modal holds; the key is spent.
    Hold,
}

/// Resolves one key press against the modal.
pub fn decide(key: &Key) -> Decision {
    match key {
        Key::Named(NamedKey::Enter) => Decision::Save,
        Key::Named(NamedKey::Escape) => Decision::Cancel,
        Key::Character(c) if c.eq_ignore_ascii_case("d") => Decision::Discard,
        _ => Decision::Hold,
    }
}

/// The centered modal: the question and its three answers, in the
/// theme's overlay colors.
pub fn draw(
    painter: &mut crate::paint::painter::Painter,
    theme: &crate::style::theme::Theme,
    width: f32,
    height: f32,
) {
    use crate::style::fonts::BODY_FAMILY;
    const SIZE: f32 = 15.0;
    const PAD: f32 = 24.0;
    const RADIUS: f32 = 12.0;
    let title = "Unsaved changes";
    let answers = "Enter saves, D discards, Esc cancels";
    let text_w = painter
        .measure(title, BODY_FAMILY, SIZE, 600)
        .max(painter.measure(answers, BODY_FAMILY, SIZE, 400));
    let w = text_w + 2.0 * PAD;
    let h = 2.0 * PAD + 2.0 * SIZE + 18.0;
    let x = (width - w) / 2.0;
    let y = (height - h) / 2.5;
    for (grow, shadow) in [(6.0, 16.0), (3.0, 28.0)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            w + 2.0 * grow,
            h + 2.0 * grow,
            RADIUS + grow,
            crate::style::theme::Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: shadow as u8,
            },
        );
    }
    painter.fill(x, y, w, h, RADIUS, theme.ui.overlay_bg);
    painter.stroke(x, y, w, h, RADIUS, 1.0, theme.blocks.table_border);
    painter.text(
        x + PAD,
        y + PAD,
        title,
        BODY_FAMILY,
        SIZE,
        600,
        theme.ui.overlay_fg,
    );
    painter.text(
        x + PAD,
        y + PAD + SIZE + 14.0,
        answers,
        BODY_FAMILY,
        SIZE,
        400,
        theme.ui.overlay_fg,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_answers_resolve() {
        assert_eq!(decide(&Key::Named(NamedKey::Enter)), Decision::Save);
        assert_eq!(decide(&Key::Character("d".into())), Decision::Discard);
        assert_eq!(
            decide(&Key::Character("D".into())),
            Decision::Discard,
            "shift makes no difference"
        );
        assert_eq!(decide(&Key::Named(NamedKey::Escape)), Decision::Cancel);
    }

    #[test]
    fn every_other_key_is_spent_by_the_modal() {
        assert_eq!(decide(&Key::Character("x".into())), Decision::Hold);
        assert_eq!(decide(&Key::Named(NamedKey::Space)), Decision::Hold);
        assert_eq!(decide(&Key::Named(NamedKey::F5)), Decision::Hold);
    }
}

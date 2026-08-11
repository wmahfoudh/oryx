//! Transient corner notices: quiet messages that hold, fade, and leave.
//!
//! The indicator kit is deliberately minimal; the notice is its only
//! transient piece. It holds at full strength, fades on the timer, and
//! is gone. It never takes a key and never moves the page.

use std::time::{Duration, Instant};

use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};

/// Full-strength time before the fade starts.
pub const HOLD: Duration = Duration::from_secs(2);
/// The fade's length.
pub const FADE: Duration = Duration::from_millis(400);

pub struct Notice {
    text: String,
    born: Instant,
}

impl Notice {
    pub fn new(text: impl Into<String>, now: Instant) -> Notice {
        Notice {
            text: text.into(),
            born: now,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The next instant the notice needs the loop: the fade's start
    /// while holding, else the next fade tick.
    pub fn wake(&self, now: Instant) -> Instant {
        if now.saturating_duration_since(self.born) < HOLD {
            self.born + HOLD
        } else {
            now + Duration::from_millis(16)
        }
    }

    /// The notice's strength at `now`: full through the hold, sinking
    /// through the fade, `None` once it is spent.
    pub fn alpha(&self, now: Instant) -> Option<f32> {
        let age = now.saturating_duration_since(self.born);
        if age <= HOLD {
            Some(1.0)
        } else if age < HOLD + FADE {
            Some(1.0 - (age - HOLD).as_secs_f32() / FADE.as_secs_f32())
        } else {
            None
        }
    }
}

/// The floating pill over the document's bottom-right corner, in the
/// theme's overlay colors, scaled by the fade's alpha.
pub fn draw(painter: &mut Painter, theme: &Theme, text: &str, alpha: f32, width: f32, height: f32) {
    const SIZE: f32 = 14.0;
    const PAD: f32 = 14.0;
    const MARGIN: f32 = 16.0;
    const HEIGHT: f32 = 36.0;
    const RADIUS: f32 = 18.0;
    let fade = |c: Rgba| Rgba {
        a: (c.a as f32 * alpha) as u8,
        ..c
    };
    let text_w = painter.measure(text, BODY_FAMILY, SIZE, 400);
    let w = text_w + 2.0 * PAD;
    let x = (width - w - MARGIN).max(MARGIN);
    let y = height - HEIGHT - MARGIN;
    for (grow, shadow) in [(6.0, 16.0), (3.0, 28.0)] {
        painter.fill(
            x - grow,
            y - grow + 1.5,
            w + 2.0 * grow,
            HEIGHT + 2.0 * grow,
            RADIUS + grow,
            Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: (shadow * alpha) as u8,
            },
        );
    }
    painter.fill(x, y, w, HEIGHT, RADIUS, fade(theme.ui.overlay_bg));
    painter.stroke(
        x,
        y,
        w,
        HEIGHT,
        RADIUS,
        1.0,
        fade(theme.blocks.table_border),
    );
    painter.text(
        x + PAD,
        y + 9.0,
        text,
        BODY_FAMILY,
        SIZE,
        400,
        fade(theme.ui.overlay_fg),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wake_names_the_fade_start_then_ticks() {
        let t0 = Instant::now();
        let notice = Notice::new("saved", t0);
        assert_eq!(notice.wake(t0), t0 + HOLD, "holding waits for the fade");
        let mid = t0 + HOLD + FADE / 4;
        assert_eq!(
            notice.wake(mid),
            mid + Duration::from_millis(16),
            "fading ticks at frame pace"
        );
    }

    #[test]
    fn a_notice_holds_full_then_fades_then_is_spent() {
        let t0 = Instant::now();
        let notice = Notice::new("saved", t0);
        assert_eq!(notice.alpha(t0), Some(1.0));
        assert_eq!(
            notice.alpha(t0 + HOLD - Duration::from_millis(1)),
            Some(1.0),
            "the hold keeps full strength"
        );
        let early = notice.alpha(t0 + HOLD + FADE / 4).expect("still fading");
        let late = notice.alpha(t0 + HOLD + FADE / 2).expect("still fading");
        assert!(
            early > 0.0 && early < 1.0,
            "the fade sinks below full: {early}"
        );
        assert!(late < early, "the fade descends: {early} then {late}");
        assert_eq!(
            notice.alpha(t0 + HOLD + FADE + Duration::from_millis(1)),
            None,
            "a spent notice is gone"
        );
    }
}

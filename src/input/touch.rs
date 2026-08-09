//! Touch gestures over raw `WindowEvent::Touch` sequences: one finger
//! pans and flings, two fingers pinch. Positions are physical pixels;
//! thresholds scale with the display so a tap feels the same everywhere.
//!
//! Taps are reported but Windows also emulates a mouse click for them,
//! so the app only acts on the report where no emulation exists.

use std::time::{Duration, Instant};

/// Movement below this many logical units stays a tap.
pub const SLOP: f32 = 8.0;
/// Release velocity below this many logical units per second lands the
/// page instead of flinging it.
pub const FLING_MIN: f32 = 150.0;
/// Friction retires a fling once it slows under this.
const FLING_STOP: f32 = 30.0;
/// Exponential friction time constant, in seconds.
const TAU: f32 = 0.325;
/// A finger resting this long before lifting has spent its momentum.
const REST: Duration = Duration::from_millis(100);
/// Span drift below this fraction is grip jitter, not a pinch yet.
const PINCH_SLOP: f32 = 0.05;

/// What one touch event amounts to.
pub enum Act {
    None,
    /// The finger crossed the slop: a swipe begins here.
    PanStart,
    /// Scroll delta, positive scrolling down.
    Pan {
        dy: f32,
    },
    /// The finger lifted without ever crossing the slop.
    Tap {
        x: f32,
        y: f32,
    },
    /// A swipe released with momentum, in physical pixels per second.
    Fling {
        velocity: f32,
    },
    /// A second finger landed.
    PinchStart,
    /// Span ratio against where the pinch went live.
    Pinch {
        factor: f32,
    },
    /// The gesture is over with nothing more to do.
    End,
}

enum State {
    Idle,
    /// Finger down, tap until proven otherwise.
    Pending {
        id: u64,
        start: (f32, f32),
        last: (f32, f32),
    },
    Panning {
        id: u64,
        start: (f32, f32),
        last_y: f32,
        at: Instant,
        velocity: f32,
    },
    Pinching {
        a: (u64, (f32, f32)),
        b: (u64, (f32, f32)),
        start: (f32, f32),
        /// Finger span the live factor is measured against; the base
        /// rebases when the dead zone is crossed so factors walk on
        /// from 1.0 without a jump.
        base: f32,
        live: bool,
    },
}

pub struct Tracker {
    state: State,
    scale: f32,
}

impl Tracker {
    pub fn new(scale: f32) -> Tracker {
        Tracker {
            state: State::Idle,
            scale,
        }
    }

    /// Where the active gesture began, for routing it to a pane.
    pub fn start(&self) -> Option<(f32, f32)> {
        match &self.state {
            State::Idle => None,
            State::Pending { start, .. }
            | State::Panning { start, .. }
            | State::Pinching { start, .. } => Some(*start),
        }
    }

    /// Whether a pan or pinch owns the pointer, muting emulated mouse
    /// events for its duration.
    pub fn panning(&self) -> bool {
        matches!(self.state, State::Panning { .. } | State::Pinching { .. })
    }

    pub fn on(&mut self, id: u64, phase: Phase, x: f32, y: f32, t: Instant) -> Act {
        match phase {
            Phase::Started => self.started(id, x, y),
            Phase::Moved => self.moved(id, x, y, t),
            Phase::Ended | Phase::Cancelled => self.ended(id, t, phase),
        }
    }

    fn started(&mut self, id: u64, x: f32, y: f32) -> Act {
        match self.state {
            State::Idle => {
                self.state = State::Pending {
                    id,
                    start: (x, y),
                    last: (x, y),
                };
                Act::None
            }
            // A second finger joins: whatever the first was doing
            // becomes a pinch anchored on both.
            State::Pending {
                id: held,
                start,
                last,
            } => {
                self.state = State::Pinching {
                    a: (held, last),
                    b: (id, (x, y)),
                    start,
                    base: dist(last, (x, y)),
                    live: false,
                };
                Act::PinchStart
            }
            State::Panning {
                id: held, start, ..
            } => {
                // The pan's x never mattered, so anchor the first
                // finger at the new finger's height.
                self.state = State::Pinching {
                    a: (held, (start.0, y)),
                    b: (id, (x, y)),
                    start,
                    base: dist((start.0, y), (x, y)),
                    live: false,
                };
                Act::PinchStart
            }
            // A third finger changes nothing.
            State::Pinching { .. } => Act::None,
        }
    }

    fn moved(&mut self, id: u64, x: f32, y: f32, t: Instant) -> Act {
        match &mut self.state {
            State::Pending {
                id: held,
                start,
                last,
            } if *held == id => {
                *last = (x, y);
                let start = *start;
                if dist(start, (x, y)) <= SLOP * self.scale {
                    return Act::None;
                }
                self.state = State::Panning {
                    id,
                    start,
                    last_y: y,
                    at: t,
                    velocity: 0.0,
                };
                Act::PanStart
            }
            State::Panning {
                id: held,
                last_y,
                at,
                velocity,
                ..
            } if *held == id => {
                let dy = *last_y - y;
                let dt = t.duration_since(*at).as_secs_f32();
                if dt > 0.0 {
                    let instant = dy / dt;
                    *velocity = 0.25 * *velocity + 0.75 * instant;
                }
                *last_y = y;
                *at = t;
                Act::Pan { dy }
            }
            State::Pinching {
                a, b, base, live, ..
            } => {
                if a.0 == id {
                    a.1 = (x, y);
                } else if b.0 == id {
                    b.1 = (x, y);
                } else {
                    return Act::None;
                }
                let span = dist(a.1, b.1).max(1.0);
                if !*live {
                    if (span / *base - 1.0).abs() > PINCH_SLOP {
                        *live = true;
                        *base = span;
                    }
                    return Act::None;
                }
                Act::Pinch {
                    factor: span / *base,
                }
            }
            _ => Act::None,
        }
    }

    fn ended(&mut self, id: u64, t: Instant, phase: Phase) -> Act {
        match &self.state {
            State::Pending {
                id: held, start, ..
            } if *held == id => {
                let (x, y) = *start;
                self.state = State::Idle;
                if matches!(phase, Phase::Cancelled) {
                    Act::End
                } else {
                    Act::Tap { x, y }
                }
            }
            State::Panning {
                id: held,
                at,
                velocity,
                ..
            } if *held == id => {
                let rested = t.duration_since(*at) > REST;
                let velocity = *velocity;
                self.state = State::Idle;
                let flings = !rested
                    && !matches!(phase, Phase::Cancelled)
                    && velocity.abs() >= FLING_MIN * self.scale;
                if flings {
                    Act::Fling { velocity }
                } else {
                    Act::End
                }
            }
            State::Pinching { a, b, .. } if a.0 == id || b.0 == id => {
                self.state = State::Idle;
                Act::End
            }
            _ => Act::None,
        }
    }
}

/// Advances a fling by `dt` seconds: the scroll delta to apply and the
/// velocity friction leaves behind.
pub fn fling_step(velocity: f32, dt: f32) -> (f32, f32) {
    (velocity * dt, velocity * (-dt / TAU).exp())
}

/// Whether friction has retired the fling.
pub fn fling_done(velocity: f32, scale: f32) -> bool {
    velocity.abs() < FLING_STOP * scale
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Touch phases, mirroring winit's.
#[derive(Clone, Copy)]
pub enum Phase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn ms(t0: Instant, millis: u64) -> Instant {
        t0 + Duration::from_millis(millis)
    }

    #[test]
    fn a_still_finger_taps_at_its_origin() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        assert!(matches!(
            touch.on(5, Phase::Started, 100.0, 200.0, t0),
            Act::None
        ));
        assert!(matches!(
            touch.on(5, Phase::Moved, 102.0, 203.0, ms(t0, 16)),
            Act::None
        ));
        let act = touch.on(5, Phase::Ended, 102.0, 203.0, ms(t0, 80));
        assert!(matches!(act, Act::Tap { x, y } if x == 100.0 && y == 200.0));
    }

    #[test]
    fn crossing_the_slop_starts_a_pan() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        let act = touch.on(1, Phase::Moved, 100.0, 480.0, ms(t0, 16));
        assert!(matches!(act, Act::PanStart));
        assert_eq!(touch.start(), Some((100.0, 500.0)));
        let act = touch.on(1, Phase::Moved, 100.0, 450.0, ms(t0, 32));
        assert!(matches!(act, Act::Pan { dy } if dy == 30.0));
    }

    #[test]
    fn slop_scales_with_the_display() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(2.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        assert!(matches!(
            touch.on(1, Phase::Moved, 100.0, 490.0, ms(t0, 16)),
            Act::None
        ));
        let act = touch.on(1, Phase::Ended, 100.0, 490.0, ms(t0, 60));
        assert!(matches!(act, Act::Tap { .. }));
    }

    #[test]
    fn a_quick_release_flings() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        touch.on(1, Phase::Moved, 100.0, 470.0, ms(t0, 16));
        touch.on(1, Phase::Moved, 100.0, 440.0, ms(t0, 32));
        touch.on(1, Phase::Moved, 100.0, 410.0, ms(t0, 48));
        let act = touch.on(1, Phase::Ended, 100.0, 410.0, ms(t0, 58));
        assert!(matches!(act, Act::Fling { velocity } if velocity > FLING_MIN));
    }

    #[test]
    fn a_rested_finger_does_not_fling() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        touch.on(1, Phase::Moved, 100.0, 470.0, ms(t0, 16));
        touch.on(1, Phase::Moved, 100.0, 440.0, ms(t0, 32));
        let act = touch.on(1, Phase::Ended, 100.0, 440.0, ms(t0, 300));
        assert!(matches!(act, Act::End));
    }

    #[test]
    fn a_cancelled_gesture_neither_taps_nor_flings() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        assert!(matches!(
            touch.on(1, Phase::Cancelled, 100.0, 500.0, ms(t0, 16)),
            Act::End
        ));
        touch.on(2, Phase::Started, 100.0, 500.0, ms(t0, 40));
        touch.on(2, Phase::Moved, 100.0, 400.0, ms(t0, 56));
        let act = touch.on(2, Phase::Cancelled, 100.0, 380.0, ms(t0, 64));
        assert!(matches!(act, Act::End));
    }

    #[test]
    fn a_second_finger_starts_a_pinch() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 100.0, t0);
        let act = touch.on(2, Phase::Started, 200.0, 100.0, ms(t0, 16));
        assert!(matches!(act, Act::PinchStart));
        assert!(touch.panning(), "a pinch mutes emulated mouse input");
    }

    #[test]
    fn pinch_factor_tracks_the_span() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 100.0, t0);
        touch.on(2, Phase::Started, 200.0, 100.0, ms(t0, 16));
        // Crossing the pinch dead zone rebases, so the factor walks on
        // from 1.0 without a jump.
        let act = touch.on(2, Phase::Moved, 250.0, 100.0, ms(t0, 32));
        assert!(matches!(act, Act::None));
        let act = touch.on(2, Phase::Moved, 400.0, 100.0, ms(t0, 48));
        assert!(matches!(act, Act::Pinch { factor } if (factor - 2.0).abs() < 0.01));
    }

    #[test]
    fn lifting_a_pinch_finger_ends_the_gesture() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 100.0, t0);
        touch.on(2, Phase::Started, 200.0, 100.0, ms(t0, 16));
        let act = touch.on(1, Phase::Ended, 100.0, 100.0, ms(t0, 32));
        assert!(matches!(act, Act::End));
        assert!(matches!(
            touch.on(2, Phase::Moved, 220.0, 100.0, ms(t0, 48)),
            Act::None
        ));
    }

    #[test]
    fn a_pan_becomes_a_pinch_when_the_second_finger_lands() {
        let t0 = Instant::now();
        let mut touch = Tracker::new(1.0);
        touch.on(1, Phase::Started, 100.0, 500.0, t0);
        touch.on(1, Phase::Moved, 100.0, 450.0, ms(t0, 16));
        let act = touch.on(2, Phase::Started, 200.0, 450.0, ms(t0, 32));
        assert!(matches!(act, Act::PinchStart));
    }

    #[test]
    fn fling_decays_toward_rest() {
        let (delta, after) = fling_step(1000.0, 0.016);
        assert!((delta - 16.0).abs() < 0.01);
        assert!(after < 1000.0 && after > 0.0);
        let mut velocity = 1000.0;
        for _ in 0..200 {
            velocity = fling_step(velocity, 0.016).1;
        }
        assert!(fling_done(velocity, 1.0));
        assert!(!fling_done(1000.0, 1.0));
    }
}

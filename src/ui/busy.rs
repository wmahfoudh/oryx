//! The busy dot: a small round mark in the page's bottom right corner
//! while Oryx works in the background, and nothing when it is idle.
//! This module says when it shows. `ui::wordcount` draws it, since the
//! corner is the word count's.

use std::time::{Duration, Instant};

/// How long work must have lasted before the dot shows, so that a key
/// in a small file does not flash it.
pub const DELAY: Duration = Duration::from_millis(300);

/// How long a dot that went on stays, so that work which ends right
/// after the delay shows a dot the eye can read, not a flash.
pub const LEAST: Duration = Duration::from_millis(300);

/// How often the loop asks again while work runs. The end of the
/// coloring has no sure wake: its thread marks itself done after its
/// last one.
pub const CHECK: Duration = Duration::from_millis(200);

/// The work in the background, a field per kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Work {
    /// The parse worker owes a document.
    pub parse: bool,
    /// The layout pass has blocks left to place.
    pub measure: bool,
    /// The coloring's sweep runs.
    pub coloring: bool,
    /// A word count is out.
    pub count: bool,
    /// A PDF export runs.
    pub export: bool,
}

impl Work {
    pub fn any(self) -> bool {
        self.parse || self.measure || self.coloring || self.count || self.export
    }
}

/// Since when work has run without a stop, and until when a dot that
/// went on stays after its work.
#[derive(Debug, Default)]
pub struct Busy {
    since: Option<Instant>,
    until: Option<Instant>,
}

impl Busy {
    /// Takes the answer of the moment: work that goes on keeps its
    /// start, and work that stopped forgets it. Work that stops with
    /// its dot on for less than `LEAST` leaves the dot its time.
    pub fn note(&mut self, working: bool, now: Instant) {
        if self.until.is_some_and(|until| until <= now) {
            self.until = None;
        }
        if working {
            self.since.get_or_insert(now);
        } else if let Some(since) = self.since.take() {
            let lit = since + DELAY;
            if lit <= now && now < lit + LEAST {
                self.until = Some(lit + LEAST);
            }
        }
    }

    /// Whether the dot shows at `now`.
    pub fn shows(&self, now: Instant) -> bool {
        self.since
            .is_some_and(|since| now.saturating_duration_since(since) >= DELAY)
            || self.until.is_some_and(|until| now < until)
    }

    /// The next instant the loop is needed: the end of the delay, then
    /// the next check, and the end of a dot's least time. None when
    /// idle with no dot.
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let work = self.since.map(|since| {
            let lit = since + DELAY;
            if now < lit {
                lit
            } else {
                now + CHECK
            }
        });
        let stay = self.until.filter(|until| now < *until);
        match (work, stay) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_work_is_work() {
        assert!(!Work::default().any());
        let kinds = [
            Work {
                parse: true,
                ..Work::default()
            },
            Work {
                measure: true,
                ..Work::default()
            },
            Work {
                coloring: true,
                ..Work::default()
            },
            Work {
                count: true,
                ..Work::default()
            },
            Work {
                export: true,
                ..Work::default()
            },
        ];
        for work in kinds {
            assert!(work.any(), "{work:?}");
        }
    }

    #[test]
    fn the_dot_shows_once_work_has_lasted_the_delay() {
        let start = Instant::now();
        let mut busy = Busy::default();
        assert!(!busy.shows(start));
        busy.note(true, start);
        assert!(!busy.shows(start));
        assert!(!busy.shows(start + DELAY - Duration::from_millis(1)));
        assert!(busy.shows(start + DELAY));
        // Work that goes on keeps its start.
        busy.note(true, start + DELAY);
        assert!(busy.shows(start + DELAY));
    }

    #[test]
    fn work_that_stops_before_the_delay_shows_no_dot() {
        let start = Instant::now();
        let mut busy = Busy::default();
        busy.note(true, start);
        busy.note(false, start + Duration::from_millis(100));
        assert!(!busy.shows(start + DELAY));
        // Work that starts again waits the whole delay again.
        let again = start + Duration::from_millis(200);
        busy.note(true, again);
        assert!(!busy.shows(start + DELAY));
        assert!(busy.shows(again + DELAY));
    }

    #[test]
    fn the_dot_goes_out_when_the_work_ends() {
        let start = Instant::now();
        let mut busy = Busy::default();
        busy.note(true, start);
        let later = start + Duration::from_secs(5);
        assert!(busy.shows(later));
        busy.note(false, later);
        assert!(!busy.shows(later));
    }

    #[test]
    fn a_dot_that_showed_stays_a_least_time() {
        let start = Instant::now();
        let mut busy = Busy::default();
        busy.note(true, start);
        // The work ends 100 ms after the dot went on.
        let end = start + DELAY + Duration::from_millis(100);
        busy.note(false, end);
        let out = start + DELAY + LEAST;
        assert!(busy.shows(end));
        assert!(busy.shows(out - Duration::from_millis(1)));
        assert!(!busy.shows(out));
        assert_eq!(busy.wake(end), Some(out));
        busy.note(false, out);
        assert_eq!(busy.wake(out), None);
        // Work that ended before the dot went on leaves nothing.
        busy.note(true, out);
        busy.note(false, out + Duration::from_millis(100));
        assert!(!busy.shows(out + Duration::from_millis(100)));
        assert_eq!(busy.wake(out + Duration::from_millis(100)), None);
    }

    #[test]
    fn the_loop_wakes_at_the_delay_then_at_each_check_and_not_when_idle() {
        let start = Instant::now();
        let mut busy = Busy::default();
        assert_eq!(busy.wake(start), None);
        busy.note(true, start);
        assert_eq!(busy.wake(start), Some(start + DELAY));
        let before = start + Duration::from_millis(100);
        assert_eq!(busy.wake(before), Some(start + DELAY));
        let shown = start + DELAY;
        assert_eq!(busy.wake(shown), Some(shown + CHECK));
        let later = start + Duration::from_secs(5);
        assert_eq!(busy.wake(later), Some(later + CHECK));
        busy.note(false, later);
        assert_eq!(busy.wake(later), None);
    }
}

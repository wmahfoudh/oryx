//! Autosave: the rule that decides whether an automatic save may
//! write, and the pause after the last edit that one of the two
//! settings waits for. Pure, so both are testable without a window;
//! `App` owns the wiring.

use std::time::{Duration, Instant, SystemTime};

/// The pauses the settings row offers, in seconds: off, then from five
/// seconds to a quarter of an hour.
pub const PAUSES: [u32; 8] = [0, 5, 15, 30, 60, 300, 600, 900];

/// The longest pause, which a hand-written config value is held to.
pub const PAUSE_MAX: u32 = PAUSES[PAUSES.len() - 1];

/// The rest after the last edit before the untitled note's text is
/// copied to its file: between Vim's swap file (4 s) and Notepad++'s
/// backup (7 s).
pub const NOTE_REST: Duration = Duration::from_secs(5);

/// The rest for a note of `bytes`: `NOTE_REST`, and never less than a
/// second per megabyte, so a huge paste is not rewritten whole every
/// few seconds. A typed note never reaches the second term.
pub fn copy_rest(bytes: usize) -> Duration {
    NOTE_REST.max(Duration::from_secs((bytes >> 20) as u64))
}

/// What an automatic save does at its moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing is unsaved, or the text has no file of its own to go to.
    Nothing,
    /// The file on disk is no longer the one Oryx read or wrote: the
    /// write waits for the reader's own Ctrl+S.
    Hold,
    Write,
}

/// What the rule reads. `seen` is the file's identity (modified time
/// and length) recorded at the last read or write, `now` what the disk
/// answers at this moment.
#[derive(Debug, Clone, Copy)]
pub struct Facts {
    pub unsaved: bool,
    pub note: bool,
    pub deleted: bool,
    /// The reader was already told the file changed on disk.
    pub conflict: bool,
    /// The file was not on disk at its open and has not been saved yet:
    /// `oryx notes.md` on a name that did not exist.
    pub new: bool,
    pub seen: Option<(SystemTime, u64)>,
    pub now: Option<(SystemTime, u64)>,
}

/// An automatic save never overwrites someone else's change: it writes
/// only while the file on disk is provably the one last read or
/// written. Ctrl+S is the reader's word and is not bound by this.
pub fn verdict(facts: Facts) -> Verdict {
    if !facts.unsaved || facts.note {
        return Verdict::Nothing;
    }
    // A new file is untouched while nothing stands at its path: the
    // write creates it. One that appeared there is someone else's.
    let untouched = if facts.new {
        facts.now.is_none()
    } else {
        facts.seen.is_some() && facts.seen == facts.now
    };
    if facts.deleted || facts.conflict || !untouched {
        return Verdict::Hold;
    }
    Verdict::Write
}

/// The pause save's deadline. Every edit moves it, so a burst of typing
/// costs one write, and it is gone once taken, so an idle window has
/// nothing to wake for.
#[derive(Debug, Default)]
pub struct Pause {
    at: Option<Instant>,
}

impl Pause {
    /// An edit landed at `now`; `seconds` is the setting, 0 for off.
    pub fn edited(&mut self, now: Instant, seconds: u32) {
        self.at = (seconds > 0).then(|| now + Duration::from_secs(u64::from(seconds)));
    }

    /// An edit landed at `now` and the write waits for `rest`.
    pub fn rest(&mut self, now: Instant, rest: Duration) {
        self.at = Some(now + rest);
    }

    /// When the loop should wake for the save, if at all.
    pub fn wake(&self) -> Option<Instant> {
        self.at
    }

    /// True once, when the pause has lasted: the deadline is taken.
    pub fn take_due(&mut self, now: Instant) -> bool {
        let due = self.at.is_some_and(|at| now >= at);
        if due {
            self.at = None;
        }
        due
    }

    pub fn clear(&mut self) {
        self.at = None;
    }
}

/// The settings row's value: "off" at 0, whole minutes in minutes, any
/// other number in seconds.
pub fn pause_label(seconds: u32) -> String {
    match seconds {
        0 => "off".to_string(),
        1 => "1 second".to_string(),
        60 => "1 minute".to_string(),
        n if n % 60 == 0 => format!("{} minutes", n / 60),
        n => format!("{n} seconds"),
    }
}

/// One step of the settings row: the next choice above or below the
/// value, which a hand-written config may have set between two of them.
/// The ends hold.
pub fn step_pause(seconds: u32, delta: i32) -> u32 {
    let next = if delta > 0 {
        PAUSES.iter().copied().find(|&p| p > seconds)
    } else {
        PAUSES.iter().rev().copied().find(|&p| p < seconds)
    };
    next.unwrap_or(seconds.min(PAUSE_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(secs: u64, len: u64) -> Option<(SystemTime, u64)> {
        Some((SystemTime::UNIX_EPOCH + Duration::from_secs(secs), len))
    }

    fn clean_edit() -> Facts {
        Facts {
            unsaved: true,
            note: false,
            deleted: false,
            conflict: false,
            new: false,
            seen: state(100, 12),
            now: state(100, 12),
        }
    }

    /// The read of Task 359: a new file is a named file, kept by
    /// autosave as any other, so its first automatic save creates it. A
    /// file that appeared on disk meanwhile is someone else's: held.
    #[test]
    fn a_new_file_is_created_by_its_first_automatic_save() {
        let new = Facts {
            new: true,
            seen: None,
            now: None,
            ..clean_edit()
        };
        assert_eq!(verdict(new), Verdict::Write);
        let appeared = Facts {
            now: state(100, 12),
            ..new
        };
        assert_eq!(verdict(appeared), Verdict::Hold);
        let nothing_typed = Facts {
            unsaved: false,
            ..new
        };
        assert_eq!(verdict(nothing_typed), Verdict::Nothing);
    }

    #[test]
    fn unsaved_edits_over_an_untouched_file_are_written() {
        assert_eq!(verdict(clean_edit()), Verdict::Write);
    }

    #[test]
    fn nothing_unsaved_writes_nothing() {
        let facts = Facts {
            unsaved: false,
            ..clean_edit()
        };
        assert_eq!(verdict(facts), Verdict::Nothing);
    }

    #[test]
    fn the_note_is_left_alone() {
        let facts = Facts {
            note: true,
            ..clean_edit()
        };
        assert_eq!(verdict(facts), Verdict::Nothing);
    }

    #[test]
    fn a_file_changed_on_disk_holds_the_write() {
        let newer = Facts {
            now: state(160, 12),
            ..clean_edit()
        };
        assert_eq!(verdict(newer), Verdict::Hold, "another modified time");
        let longer = Facts {
            now: state(100, 40),
            ..clean_edit()
        };
        assert_eq!(verdict(longer), Verdict::Hold, "another length");
    }

    #[test]
    fn a_change_the_reader_was_told_about_keeps_holding() {
        // The disk check records the new identity when it tells the
        // reader, so the two states agree again; the flag keeps the hold
        // until Ctrl+S or a reload.
        let facts = Facts {
            conflict: true,
            seen: state(160, 12),
            now: state(160, 12),
            ..clean_edit()
        };
        assert_eq!(verdict(facts), Verdict::Hold);
    }

    #[test]
    fn a_file_gone_from_disk_is_not_written_back() {
        let missing = Facts {
            now: None,
            ..clean_edit()
        };
        assert_eq!(verdict(missing), Verdict::Hold, "missing at this moment");
        let declared = Facts {
            deleted: true,
            now: None,
            ..clean_edit()
        };
        assert_eq!(verdict(declared), Verdict::Hold, "declared deleted");
    }

    #[test]
    fn an_identity_never_recorded_holds_the_write() {
        let facts = Facts {
            seen: None,
            ..clean_edit()
        };
        assert_eq!(verdict(facts), Verdict::Hold);
    }

    #[test]
    fn an_edit_arms_the_pause_and_the_setting_at_zero_does_not() {
        let t0 = Instant::now();
        let mut pause = Pause::default();
        assert_eq!(pause.wake(), None, "nothing to wake for before an edit");
        pause.edited(t0, 0);
        assert_eq!(pause.wake(), None, "off");
        pause.edited(t0, 5);
        assert_eq!(pause.wake(), Some(t0 + Duration::from_secs(5)));
    }

    #[test]
    fn a_burst_of_typing_costs_one_write() {
        let t0 = Instant::now();
        let mut pause = Pause::default();
        let mut writes = 0;
        // A key every 200 ms for four seconds, the loop waking between.
        for tick in 0..20 {
            let now = t0 + Duration::from_millis(200 * tick);
            if pause.take_due(now) {
                writes += 1;
            }
            pause.edited(now, 2);
        }
        assert_eq!(writes, 0, "no write mid-burst");
        let last = t0 + Duration::from_millis(200 * 19);
        assert!(!pause.take_due(last + Duration::from_millis(1999)));
        assert!(pause.take_due(last + Duration::from_secs(2)));
        assert_eq!(pause.wake(), None, "nothing wakes the loop after the write");
        assert!(!pause.take_due(last + Duration::from_secs(60)));
    }

    #[test]
    fn turning_the_setting_off_drops_a_standing_deadline() {
        let t0 = Instant::now();
        let mut pause = Pause::default();
        pause.edited(t0, 5);
        pause.edited(t0 + Duration::from_secs(1), 0);
        assert_eq!(pause.wake(), None);
    }

    #[test]
    fn a_typed_note_rests_five_seconds_and_a_huge_one_a_second_per_megabyte() {
        assert_eq!(copy_rest(0), Duration::from_secs(5));
        assert_eq!(copy_rest(4096), Duration::from_secs(5));
        assert_eq!(copy_rest(5 << 20), Duration::from_secs(5));
        assert_eq!(copy_rest(8 << 20), Duration::from_secs(8));
        assert_eq!(copy_rest(100 << 20), Duration::from_secs(100));
    }

    #[test]
    fn a_rest_arms_the_pause_like_an_edit() {
        let t0 = Instant::now();
        let mut pause = Pause::default();
        pause.rest(t0, NOTE_REST);
        pause.rest(t0 + Duration::from_secs(1), NOTE_REST);
        assert_eq!(pause.wake(), Some(t0 + Duration::from_secs(6)));
        assert!(!pause.take_due(t0 + Duration::from_secs(5)));
        assert!(pause.take_due(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn the_row_steps_through_the_choices_and_stops_at_both_ends() {
        let mut seen = vec![0];
        for _ in 0..9 {
            seen.push(step_pause(*seen.last().unwrap(), 1));
        }
        assert_eq!(seen, [0, 5, 15, 30, 60, 300, 600, 900, 900, 900]);
        assert_eq!(step_pause(900, -1), 600);
        assert_eq!(step_pause(5, -1), 0);
        assert_eq!(step_pause(0, -1), 0);
    }

    #[test]
    fn a_hand_written_pause_steps_to_the_choices_around_it() {
        // The config file may hold any number of seconds up to the
        // longest choice, and the save honors it as written.
        assert_eq!(step_pause(2, 1), 5);
        assert_eq!(step_pause(2, -1), 0);
        assert_eq!(step_pause(90, 1), 300);
        assert_eq!(step_pause(90, -1), 60);
    }

    #[test]
    fn the_row_reads_off_seconds_or_minutes() {
        assert_eq!(pause_label(0), "off");
        assert_eq!(pause_label(5), "5 seconds");
        assert_eq!(pause_label(60), "1 minute");
        assert_eq!(pause_label(900), "15 minutes");
        assert_eq!(pause_label(1), "1 second");
        assert_eq!(pause_label(90), "90 seconds", "not a whole minute");
    }
}

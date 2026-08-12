//! The splice ledger: every edit as a replacement over the baseline.
//!
//! Entering edit mode fixes the baseline, the text the file had at open
//! or last save. Every edit is one entry in an ordered splice list,
//! replace this baseline range with these bytes; adjacent entries merge
//! as typing proceeds. Saving walks the baseline emitting untouched
//! bytes verbatim and replacements where splices sit, so the covenant
//! holds by construction and a zero-splice save is the baseline itself.
//!
//! The ledger speaks the normalized source the page displays. Line
//! endings are bytes of the file, not of the source: the positions
//! where the load normalized CRLF away are recorded, untouched line
//! endings emit exactly as they were, and a newline typed into the
//! document adopts the file's dominant ending.

use std::ops::Range;
use std::sync::Arc;

/// One replacement: `range` in baseline coordinates, `text` the bytes
/// now standing there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub range: Range<usize>,
    pub text: String,
}

pub struct Ledger {
    /// The normalized text fixed at edit entry; splice ranges index it.
    base: Arc<str>,
    /// Baseline-text offsets of newlines the load normalized from CRLF.
    crlf: Vec<u32>,
    /// Ordered by start, non-overlapping.
    splices: Vec<Splice>,
}

impl Ledger {
    pub fn new(base: Arc<str>, crlf: Vec<u32>) -> Ledger {
        Ledger {
            base,
            crlf,
            splices: Vec::new(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.splices.is_empty()
    }

    pub fn splice_count(&self) -> usize {
        self.splices.len()
    }

    /// The current-text span of each splice and the cumulative length
    /// delta standing before each; one extra delta entry closes the sum
    /// over all of them.
    fn spans(&self) -> (Vec<Range<usize>>, Vec<isize>) {
        let mut spans = Vec::with_capacity(self.splices.len());
        let mut deltas = Vec::with_capacity(self.splices.len() + 1);
        let mut delta = 0isize;
        for s in &self.splices {
            deltas.push(delta);
            let start = (s.range.start as isize + delta) as usize;
            spans.push(start..start + s.text.len());
            delta += s.text.len() as isize - s.range.len() as isize;
        }
        deltas.push(delta);
        (spans, deltas)
    }

    /// Newlines in the current text before `cur_pos`, counted over the
    /// pieces without materializing the text.
    fn newlines_before(&self, cur_pos: usize) -> usize {
        let mut count = 0;
        let mut cur = 0;
        let mut base = 0;
        let mut take = |bytes: &str, cur: &mut usize| {
            let keep = bytes.len().min(cur_pos.saturating_sub(*cur));
            count += bytes[..keep].matches('\n').count();
            *cur += bytes.len();
        };
        for s in &self.splices {
            take(&self.base[base..s.range.start], &mut cur);
            take(&s.text, &mut cur);
            base = s.range.end;
        }
        take(&self.base[base..], &mut cur);
        count
    }

    /// Replaces `range` of the current text with `text`: an insertion
    /// (empty range), a deletion (empty text), or both at once. Merges
    /// with the splices it touches; an edit undone back to the baseline
    /// leaves no entry. Returns the range of current-text line indices
    /// the edit dirtied, for the incremental relayout.
    pub fn edit(&mut self, range: Range<usize>, text: &str) -> Range<usize> {
        let start_line = self.newlines_before(range.start);
        let touched = start_line..start_line + text.matches('\n').count() + 1;

        let (a, b) = (range.start, range.end);
        let (spans, deltas) = self.spans();
        // The splices the edit touches, boundaries inclusive so typing
        // at a replacement's edge extends it instead of siding with it.
        let i0 = spans.partition_point(|sp| sp.end < a);
        let j = spans.partition_point(|sp| sp.start <= b);
        let (new_start, prefix) = if i0 < j && a >= spans[i0].start {
            let cut = a - spans[i0].start;
            (self.splices[i0].range.start, &self.splices[i0].text[..cut])
        } else {
            ((a as isize - deltas[i0]) as usize, "")
        };
        let (new_end, suffix) = if i0 < j && b <= spans[j - 1].end {
            let cut = b - spans[j - 1].start;
            (
                self.splices[j - 1].range.end,
                &self.splices[j - 1].text[cut..],
            )
        } else {
            ((b as isize - deltas[j]) as usize, "")
        };
        let merged = Splice {
            range: new_start..new_end,
            text: format!("{prefix}{text}{suffix}"),
        };
        // A splice whose text is the baseline's own is no edit: undo
        // lands these, and keeping one would read dirty and emit its
        // newlines in the dominant ending instead of their own bytes.
        let keep = merged.text != self.base[merged.range.clone()];
        self.splices.splice(i0..j, keep.then_some(merged));
        touched
    }

    /// The current text: the baseline with every splice applied.
    pub fn current(&self) -> String {
        let mut out = String::with_capacity(self.base.len());
        let mut base = 0;
        for s in &self.splices {
            out.push_str(&self.base[base..s.range.start]);
            out.push_str(&s.text);
            base = s.range.end;
        }
        out.push_str(&self.base[base..]);
        out
    }

    /// Maps a baseline offset to current coordinates, so ranges
    /// recorded against the baseline stay resolvable after edits. The
    /// bias is left: an offset at an insertion point stays before the
    /// inserted text.
    pub fn to_current(&self, base_offset: usize) -> usize {
        let mut delta = 0isize;
        for s in &self.splices {
            let past =
                s.range.end < base_offset || (s.range.end == base_offset && !s.range.is_empty());
            if !past {
                break;
            }
            delta += s.text.len() as isize - s.range.len() as isize;
        }
        (base_offset as isize + delta) as usize
    }

    /// Lines of the current text the splices touch, the save notice's
    /// figure: the union of each splice's line span, so splices sharing
    /// a line never count it twice.
    pub fn touched_lines(&self) -> usize {
        let mut count = 0;
        let mut last: Option<usize> = None;
        let (spans, _) = self.spans();
        for (splice, span) in self.splices.iter().zip(spans) {
            let start = self.newlines_before(span.start);
            let end = start + splice.text.matches('\n').count();
            let from = match last {
                Some(l) if l >= start => l + 1,
                _ => start,
            };
            count += (end + 1).saturating_sub(from);
            last = Some(end.max(last.unwrap_or(0)));
        }
        count
    }

    /// Saves: the file bytes as `emit`, with the ledger re-fixed on the
    /// written state, so the current text becomes the baseline, the
    /// splices clear, and the new CRLF positions are exactly where the
    /// emission wrote them.
    pub fn commit(&mut self) -> Vec<u8> {
        let (out, crlf) = self.render();
        self.base = Arc::from(self.current());
        self.crlf = crlf;
        self.splices.clear();
        out
    }

    /// The file bytes the current text saves as: untouched baseline
    /// bytes verbatim, normalized line endings restored where they
    /// stood, and every newline inside a replacement written in the
    /// file's dominant ending.
    pub fn emit(&self) -> Vec<u8> {
        self.render().0
    }

    /// The emission and, beside it, the current-text offsets of every
    /// newline written as CRLF: the commit's new recording.
    fn render(&self) -> (Vec<u8>, Vec<u32>) {
        let newlines = self.base.matches('\n').count();
        let dominant_crlf = self.crlf.len() * 2 > newlines;
        let mut out = Vec::with_capacity(self.base.len() + self.crlf.len());
        let mut written = Vec::with_capacity(self.crlf.len());
        // The normalized position of the next byte, walked alongside
        // the emission so CRLF positions record in current coordinates.
        let mut cur = 0;
        let mut crlf = self.crlf.iter().map(|&p| p as usize).peekable();
        let mut push_base =
            |range: Range<usize>, out: &mut Vec<u8>, written: &mut Vec<u32>, cur: &mut usize| {
                let mut at = range.start;
                while let Some(&p) = crlf.peek() {
                    if p >= range.end {
                        break;
                    }
                    if p >= at {
                        out.extend_from_slice(self.base[at..p].as_bytes());
                        out.extend_from_slice(b"\r\n");
                        written.push((*cur + p - at) as u32);
                        *cur += p - at + 1;
                        at = p + 1;
                    }
                    crlf.next();
                }
                out.extend_from_slice(self.base[at..range.end].as_bytes());
                *cur += range.end - at;
            };
        let mut base = 0;
        for s in &self.splices {
            push_base(base..s.range.start, &mut out, &mut written, &mut cur);
            // Skip the replaced range's own normalized newlines.
            for line in s.text.split_inclusive('\n') {
                if let Some(head) = line.strip_suffix('\n') {
                    out.extend_from_slice(head.as_bytes());
                    cur += head.len();
                    if dominant_crlf {
                        out.extend_from_slice(b"\r\n");
                        written.push(cur as u32);
                    } else {
                        out.push(b'\n');
                    }
                    cur += 1;
                } else {
                    out.extend_from_slice(line.as_bytes());
                    cur += line.len();
                }
            }
            base = s.range.end;
        }
        push_base(base..self.base.len(), &mut out, &mut written, &mut cur);
        (out, written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(base: &str) -> Ledger {
        Ledger::new(Arc::from(base), Vec::new())
    }

    #[test]
    fn typing_in_sequence_merges_into_one_splice() {
        let mut led = ledger("abc");
        led.edit(1..1, "x");
        led.edit(2..2, "y");
        led.edit(3..3, "z");
        assert_eq!(led.current(), "axyzbc");
        assert_eq!(led.splice_count(), 1, "adjacent typing is one entry");
    }

    #[test]
    fn distant_edits_stay_separate_entries() {
        let mut led = ledger("alpha beta gamma");
        led.edit(0..0, "x");
        led.edit(12..12, "y");
        assert_eq!(led.current(), "xalpha beta ygamma");
        assert_eq!(led.splice_count(), 2);
    }

    #[test]
    fn a_deletion_bridging_two_splices_coalesces_them() {
        let mut led = ledger("aaa bbb ccc");
        led.edit(2..2, "x");
        led.edit(6..6, "y");
        assert_eq!(led.current(), "aaxa bybb ccc");
        led.edit(1..8, "");
        assert_eq!(led.current(), "ab ccc");
        assert_eq!(led.splice_count(), 1);
    }

    #[test]
    fn reconstruction_tracks_interleaved_edits() {
        let mut led = ledger("the quick brown fox\n");
        led.edit(4..9, "slow");
        assert_eq!(led.current(), "the slow brown fox\n");
        led.edit(9..15, "");
        assert_eq!(led.current(), "the slow fox\n");
        led.edit(12..12, " jumps");
        assert_eq!(led.current(), "the slow fox jumps\n");
    }

    #[test]
    fn an_edit_undone_back_to_the_baseline_leaves_no_entry() {
        let mut led = ledger("abc");
        led.edit(1..1, "x");
        assert!(led.is_dirty());
        led.edit(1..2, "");
        assert_eq!(led.current(), "abc");
        assert!(!led.is_dirty(), "a vanished edit leaves a clean ledger");
        assert_eq!(led.splice_count(), 0);
    }

    #[test]
    fn a_replacement_restored_to_the_baseline_leaves_no_entry() {
        // Undo applies inverses as ordinary edits, so a replacement
        // undone lands a splice whose text is the baseline's own; it
        // must vanish, or the file reads dirty and emit rewrites its
        // newlines in the dominant ending.
        let mut led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5]);
        led.edit(3..8, "XX");
        assert_eq!(led.current(), "alpXXta\ngamma\n");
        led.edit(3..5, "ha\nbe");
        assert_eq!(led.current(), "alpha\nbeta\ngamma\n");
        assert!(!led.is_dirty(), "the restored text is the baseline");
        assert_eq!(
            led.emit(),
            b"alpha\r\nbeta\ngamma\n",
            "the restored newline emits as the byte it was"
        );
    }

    #[test]
    fn baseline_offsets_resolve_through_the_deltas() {
        let mut led = ledger("aaa bbb ccc");
        led.edit(4..4, "xx");
        assert_eq!(led.to_current(0), 0, "before the splice nothing moves");
        assert_eq!(
            led.to_current(4),
            4,
            "at the splice start nothing moved yet"
        );
        assert_eq!(led.to_current(7), 9, "past the splice the delta applies");
        led.edit(10..11, "");
        assert_eq!(led.to_current(9), 10, "a deletion pulls later offsets back");
        assert_eq!(led.to_current(11), 12);
    }

    #[test]
    fn a_zero_splice_save_is_the_original_bytes() {
        let led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5, 16]);
        assert_eq!(
            led.emit(),
            b"alpha\r\nbeta\ngamma\r\n",
            "normalized endings return exactly where they stood"
        );
    }

    #[test]
    fn an_edit_keeps_every_untouched_line_ending() {
        let mut led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5, 16]);
        led.edit(6..10, "BETA");
        assert_eq!(
            led.emit(),
            b"alpha\r\nBETA\ngamma\r\n",
            "only the touched line's bytes change"
        );
    }

    #[test]
    fn a_new_line_adopts_the_dominant_ending() {
        // Two of three endings are CRLF, so the file is a CRLF file.
        let mut led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5, 10]);
        led.edit(8..8, "\n");
        assert_eq!(
            led.emit(),
            b"alpha\r\nbe\r\nta\r\ngamma\n",
            "the typed newline is CRLF, the untouched LF stays LF"
        );
        // One of three is CRLF, so the file is an LF file.
        let mut led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5]);
        led.edit(8..8, "\n");
        assert_eq!(
            led.emit(),
            b"alpha\r\nbe\nta\ngamma\n",
            "the typed newline is LF, the untouched CRLF stays CRLF"
        );
    }

    #[test]
    fn touched_lines_counts_the_save_notice_figure() {
        let mut led = ledger("aaa\nbbb\nccc\nddd\n");
        assert_eq!(led.touched_lines(), 0, "a clean ledger touches nothing");
        led.edit(1..1, "x");
        assert_eq!(led.touched_lines(), 1);
        led.edit(9..9, "y");
        assert_eq!(led.touched_lines(), 2, "distant edits count their lines");
        led.edit(5..5, "one\ntwo");
        assert_eq!(
            led.touched_lines(),
            4,
            "a splice spans its start line through its last"
        );
    }

    #[test]
    fn commit_rebases_the_ledger_on_the_written_bytes() {
        // A CRLF-dominant file: the untouched ending stays LF, typed
        // newlines adopt CRLF.
        let mut led = Ledger::new(Arc::from("alpha\nbeta\ngamma\n"), vec![5, 10]);
        led.edit(6..10, "BETA");
        led.edit(8..8, "\n");
        let bytes = led.commit();
        assert_eq!(bytes, led.emit(), "commit writes what emit describes");
        assert_eq!(bytes, b"alpha\r\nBE\r\nTA\r\ngamma\n");
        assert!(!led.is_dirty(), "the written state is the baseline");
        assert_eq!(led.current(), "alpha\nBE\nTA\ngamma\n");
        // The re-fixed baseline: untouched endings emit as written,
        // and a new edit's newline still adopts the dominant ending.
        led.edit(3..3, "\n");
        assert_eq!(
            led.emit(),
            b"alp\r\nha\r\nBE\r\nTA\r\ngamma\n",
            "the committed endings are the new baseline's own"
        );
    }

    #[test]
    fn a_clean_commit_is_the_original_bytes() {
        let mut led = Ledger::new(Arc::from("alpha\nbeta\n"), vec![5]);
        assert_eq!(led.commit(), b"alpha\r\nbeta\n");
        assert_eq!(
            led.commit(),
            b"alpha\r\nbeta\n",
            "committing twice is stable"
        );
    }

    #[test]
    fn the_touched_line_range_follows_the_edit() {
        let mut led = ledger("aaa\nbbb\nccc\n");
        assert_eq!(
            led.edit(5..5, "x"),
            1..2,
            "a mid-line insertion dirties its line"
        );
        assert_eq!(
            led.edit(6..6, "\n"),
            1..3,
            "a split dirties both resulting lines"
        );
        let mut led = ledger("aaa\nbbb\nccc\n");
        assert_eq!(led.edit(3..4, ""), 0..1, "a join dirties the joined line");
    }
}

//! The seam table of a block across the keys (Task 332): what an edit, a
//! key's walk, a chunk of the worker, a caret seam and a guess do to the
//! seams, and what a resume reads from them. The app keeps the timers,
//! the asks in flight and the flush; the table keeps what a sweep can
//! trust.
//!
//! A seam is the parser state before a line. It is chained when the rows
//! from its line to the next seam, and that next seam, were computed
//! from it over the current text. A sweep that converges on a seam
//! vouches for the rows up to the first unchained seam and goes on from
//! there, so a seam that a killed sweep or a walk filed among an older
//! sweep's never stands for lines no sweep reached.

use super::highlight::{self, walk, Arrival, Filed, LineSpans, Resume, Seam};
use crate::doc::model::{BlockKind, CodeBody, Document};
use std::ops::Range;
use std::time::Duration;

/// The seam table of one block, with the lines its parse found slow.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    /// Ordered by line, one seam per line.
    seams: Vec<Filed>,
    /// Whether the rows from line 0 to the first seam were computed from
    /// a fresh start over the current text: the chained flag of the
    /// start the table has no seam for.
    head: bool,
    /// The lines whose last parse took longer than `KEY_COLOR_BUDGET`,
    /// ordered: a walk stops before one, and the worker colors it.
    slow: Vec<usize>,
}

impl Table {
    pub fn seams(&self) -> &[Filed] {
        &self.seams
    }

    pub fn slow(&self) -> &[usize] {
        &self.slow
    }

    pub fn is_empty(&self) -> bool {
        self.seams.is_empty()
    }

    /// Whether a seam stands past the frontier: a sweep resumed now could
    /// converge on it and vouch for the rows after it, so no guess is
    /// asked over them.
    pub fn past_frontier(&self, exact: usize) -> bool {
        self.seams.last().is_some_and(|f| f.line > exact)
    }

    /// Marks a line slow, as a walk or the worker found it.
    pub fn mark_slow(&mut self, line: usize) {
        if let Err(at) = self.slow.binary_search(&line) {
            self.slow.insert(at, line);
        }
    }

    fn at(&self, line: usize) -> Option<usize> {
        self.seams.binary_search_by_key(&line, |f| f.line).ok()
    }

    /// The index of the nearest seam at or before `line`.
    fn before(&self, line: usize) -> Option<usize> {
        self.seams
            .partition_point(|f| f.line <= line)
            .checked_sub(1)
    }

    /// Where a parse toward `line` starts: the nearest seam at or before
    /// it, or a fresh start at line 0.
    pub fn start_at(&self, line: usize) -> (usize, Option<Seam>) {
        match self.before(line).map(|i| &self.seams[i]) {
            Some(f) => (f.line, Some(f.seam.clone())),
            None => (0, None),
        }
    }

    fn put(&mut self, line: usize, seam: Seam, chained: bool) {
        let filed = Filed {
            line,
            seam,
            chained,
        };
        match self.seams.binary_search_by_key(&line, |f| f.line) {
            Ok(at) => self.seams[at] = filed,
            Err(at) => self.seams.insert(at, filed),
        }
    }

    /// Chains the seam a parse started from: its rows up to the next
    /// seam are that parse's. A start at line 0 with no seam is the
    /// fresh start; a start the table has no seam for chains nothing.
    fn chain(&mut self, line: usize) {
        match self.at(line) {
            Some(at) => self.seams[at].chained = true,
            None if line == 0 => self.head = true,
            None => {}
        }
    }

    /// Files the seam closing a chunk: a state the table already holds
    /// at that line keeps its flag; a new state, or a new line, is
    /// unchained, the rows after it being another parse's.
    fn file_end(&mut self, line: usize, seam: &Seam) {
        match self.at(line) {
            Some(at) if self.seams[at].seam == *seam => {}
            _ => self.put(line, seam.clone(), false),
        }
    }

    /// Files a caret seam: a line the table knows stands; a new line
    /// takes the flag of the seam before it, its rows being that seam's
    /// parse continued.
    fn file_between(&mut self, line: usize, seam: &Seam) {
        if let Some(at) = self.at(line) {
            debug_assert_eq!(
                self.seams[at].seam, *seam,
                "a caret seam at a known line carries its state"
            );
            return;
        }
        let chained = self
            .before(line)
            .map_or(self.head, |i| self.seams[i].chained);
        self.put(line, seam.clone(), chained);
    }

    /// Drops the seams strictly inside `range`, which a walk colored
    /// again from its start.
    fn drop_within(&mut self, range: Range<usize>) {
        self.seams
            .retain(|f| f.line <= range.start || f.line >= range.end);
    }

    /// Shifts the table across an edit that replaced the `old` line
    /// range with `new`. The nearest seam at or before the region's
    /// first line is unchained, its rows now holding the edit; the seams
    /// strictly inside the region describe replaced lines and drop; the
    /// seam at the region's end and the seams past it move by the line
    /// delta and keep their flags, since their states hold when the
    /// state after the edit matches, which a resumed sweep checks where
    /// it converges: a walk that reaches the region's end files the
    /// state there anew, and one that falls short, before a slow line,
    /// leaves the sweep that seam to converge on. The slow marks move
    /// the same way.
    pub fn shift(&mut self, old: &Range<usize>, new: &Range<usize>) {
        let delta = new.end as isize - old.end as isize;
        match self.before(old.start) {
            Some(at) => self.seams[at].chained = false,
            None => self.head = false,
        }
        self.seams.retain_mut(|f| {
            if f.line <= old.start {
                true
            } else if f.line < old.end {
                false
            } else {
                f.line = (f.line as isize + delta) as usize;
                true
            }
        });
        self.slow.retain_mut(|line| {
            if *line <= old.start {
                true
            } else if *line < old.end {
                false
            } else {
                *line = (*line as isize + delta) as usize;
                true
            }
        });
    }

    /// The resume of a block from its table: the sweep starts at the
    /// seam at or before the frontier and converges against the seams
    /// past it, cutting a chunk at `cuts`, the lines of the last key.
    pub fn resume(&self, exact: usize, cuts: &[usize]) -> Resume {
        let (start_line, seam) = self.start_at(exact);
        let from = self.seams.partition_point(|f| f.line <= exact);
        Resume {
            start_line,
            seam,
            expected: self.seams[from..].to_vec(),
            cuts: cuts.to_vec(),
        }
    }

    /// The coloring of a key's lines on the window's thread (Task 327,
    /// rules 1 to 4): the table shifted across the edit, then the walk
    /// from the nearest seam at or before the frontier toward the end of
    /// the touched lines, within `budget`, its spans into `highlights`
    /// before the layout shapes the touched lines. The seams the walk
    /// passed drop, its start is chained, the state before the first
    /// touched line is filed chained, and the state where it stopped is
    /// filed chained only when it equals the state stored there before
    /// the edit and that seam was chained. The lines the walk found slow
    /// are marked. The frontier follows (`frontier_after_key`). A block
    /// with no grammar is shifted and nothing else. Answers whether the
    /// lines below are owed a re-coloring by the worker.
    #[allow(clippy::too_many_arguments)]
    pub fn on_key(
        &mut self,
        source: &str,
        lines: &CodeBody,
        language: Option<&str>,
        highlights: &mut Vec<LineSpans>,
        exact: &mut usize,
        exact0: usize,
        old: &Range<usize>,
        new: &Range<usize>,
        budget: Duration,
        alive: impl FnMut() -> bool,
    ) -> bool {
        let end_before = self.at(old.end).map(|i| self.seams[i].clone());
        self.shift(old, new);
        if language.is_none() {
            debug_assert_eq!(
                *exact,
                lines.len(),
                "a block with no grammar keeps its frontier at its line count"
            );
            return false;
        }
        let (start, seam) = self.start_at(*exact);
        let walked = walk(
            source, lines, language, start, seam, new.start, new.end, budget, &self.slow, alive,
        );
        if highlights.len() < walked.reached {
            highlights.resize(walked.reached, Vec::new());
        }
        for (offset, spans) in walked.spans.into_iter().enumerate() {
            highlights[start + offset] = spans;
        }
        self.drop_within(start..walked.reached);
        self.chain(start);
        if walked.reached > new.start {
            if let Some(at_mark) = walked.at_mark {
                self.put(new.start, at_mark, true);
            }
        }
        let end_matched =
            walked.reached == new.end && end_before.as_ref().is_some_and(|f| f.seam == walked.seam);
        let chained = end_matched && end_before.is_some_and(|f| f.chained);
        self.put(walked.reached, walked.seam, chained);
        for line in walked.slow {
            self.mark_slow(line);
        }
        *exact = frontier_after_key(exact0, old, new, walked.reached, end_matched);
        self.check(*exact, highlights.len(), lines.len());
        *exact < lines.len()
    }

    /// Folds an arrival into its block. A chunk of the exact sweep
    /// writes its rows, chains its start, files its end and the slow
    /// lines it found, and advances the frontier: contiguously, and to
    /// the reach of a converged stop. A caret seam, a state and no
    /// spans, is filed when its line is still at or before the frontier,
    /// where the ask found it. A guess writes its rows at or past the
    /// frontier for display, and touches no seam. Out-of-range chunks
    /// and non-code blocks are ignored: arrivals are trusted only as far
    /// as the current document reaches.
    pub fn fold(&mut self, doc: &mut Document, arrival: &Arrival) {
        let Some(block) = doc.blocks.get_mut(arrival.block) else {
            return;
        };
        let BlockKind::CodeBlock {
            lines,
            highlights,
            exact,
            ..
        } = &mut block.kind
        else {
            return;
        };
        let end = (arrival.start_line + arrival.spans.len()).min(lines.len());
        if arrival.speculative {
            if highlights.len() < end {
                highlights.resize(end, Vec::new());
            }
            for (offset, spans) in arrival.spans.iter().enumerate() {
                let line = arrival.start_line + offset;
                if *exact <= line && line < end {
                    highlights[line] = spans.clone();
                }
            }
            return;
        }
        if arrival.spans.is_empty() {
            if let Some(seam) = &arrival.seam {
                if arrival.start_line <= *exact {
                    self.file_between(arrival.start_line, seam);
                }
            }
            self.check(*exact, highlights.len(), lines.len());
            return;
        }
        if arrival.start_line >= end {
            return;
        }
        if highlights.len() < end {
            highlights.resize(end, Vec::new());
        }
        for (offset, spans) in arrival
            .spans
            .iter()
            .enumerate()
            .take(end - arrival.start_line)
        {
            highlights[arrival.start_line + offset] = spans.clone();
        }
        if let Some(seam) = &arrival.seam {
            self.fold_chunk(arrival.start_line, end, seam, &arrival.slow);
        }
        if arrival.start_line <= *exact {
            *exact = (*exact).max(end);
        }
        if arrival.converged {
            *exact = (*exact).max(arrival.reach).min(lines.len());
        }
        self.check(*exact, highlights.len(), lines.len());
    }

    /// The seams and the slow marks of a delivered chunk, without its
    /// rows: the start chained, the end filed, the marks inside the
    /// chunk replaced by the ones its parse found.
    pub fn fold_chunk(&mut self, start: usize, end: usize, seam: &Seam, slow: &[usize]) {
        self.chain(start);
        self.file_end(end, seam);
        self.slow.retain(|line| *line < start || *line >= end);
        for line in slow {
            if (start..end).contains(line) {
                self.mark_slow(*line);
            }
        }
    }

    /// Where the worker computes a caret seam from (Task 327, rule 6):
    /// the nearest seam at or before the caret's line when it is farther
    /// than `reach` lines above, a fresh start at line 0 when there is
    /// none; nothing when a seam is within reach, or when the caret is
    /// past the frontier, where the exact sweep has not been.
    pub fn caret_ask(
        &self,
        exact: usize,
        caret_line: usize,
        reach: usize,
    ) -> Option<(usize, Option<Seam>)> {
        if caret_line > exact {
            return None;
        }
        let (start, seam) = self.start_at(caret_line);
        (caret_line - start > reach).then_some((start, seam))
    }

    /// The invariants of a table, seated where it changes: ordered, one
    /// seam per line, within the rows; the last seam unchained unless
    /// it stands at the block's end, since nothing was parsed past it;
    /// the frontier within the rows and the rows within the lines; the
    /// slow marks within the lines.
    fn check(&self, exact: usize, rows: usize, lines: usize) {
        debug_assert!(
            self.seams.windows(2).all(|w| w[0].line < w[1].line),
            "the table is ordered, one seam per line"
        );
        debug_assert!(
            self.seams.last().is_none_or(|f| f.line <= rows),
            "a seam stands within the rows"
        );
        debug_assert!(
            self.seams
                .last()
                .is_none_or(|f| !f.chained || f.line >= lines),
            "the last seam is unchained, or at the block's end"
        );
        debug_assert!(
            exact <= rows && rows <= lines,
            "the frontier within the rows ({exact} of {rows}), the rows within the lines ({lines})"
        );
        debug_assert!(
            self.slow.last().is_none_or(|line| *line < lines),
            "a slow mark stands within the lines"
        );
    }
}

/// The frontier after a key, from the frontier before it (`exact0`),
/// the touched lines before and after the edit, the line the walk
/// reached, and whether the state at the end of the touched lines is
/// the one stored there before the key. The lines before the edit stay
/// verified and the walk's lines join them; when the end state is
/// unchanged and the frontier stood past the touched lines, the lines
/// below are verified as before, moved by the edit.
pub fn frontier_after_key(
    exact0: usize,
    old: &Range<usize>,
    new: &Range<usize>,
    reached: usize,
    end_matched: bool,
) -> usize {
    let mut exact = exact0.min(new.start).max(reached);
    if end_matched && exact0 >= old.end && reached >= new.end {
        exact = exact.max(highlight::shift_line(exact0, old, new));
    }
    exact
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::load;
    use crate::edit::splice::Ledger;
    use crate::edit::splice_document;
    use crate::style::highlight::{spans, spans_chunked, Chunk};
    use std::sync::Arc;

    fn rust_doc(text: &str) -> Document {
        load::code_document(Some("rust"), text)
    }

    fn let_lines(n: usize) -> String {
        (0..n).map(|i| format!("let v{i} = {i}; // n\n")).collect()
    }

    fn parts(doc: &Document) -> (&CodeBody, Option<&str>, &Vec<LineSpans>, usize) {
        match &doc.blocks[0].kind {
            BlockKind::CodeBlock {
                lines,
                language,
                highlights,
                exact,
            } => (lines, language.as_deref(), highlights, *exact),
            _ => panic!("a code file is one code block"),
        }
    }

    fn full(doc: &Document) -> Vec<LineSpans> {
        let (lines, language, _, _) = parts(doc);
        spans(&doc.source, lines, language)
    }

    fn lines_of(table: &Table) -> Vec<(usize, bool)> {
        table.seams().iter().map(|f| (f.line, f.chained)).collect()
    }

    /// The worker in miniature: the sweep from `resume` (fresh from
    /// line 0 without one), its chunks folded in order as they are
    /// delivered, killed after `deliver` chunks when given. Answers the
    /// chunks delivered.
    fn sweep(
        doc: &mut Document,
        table: &mut Table,
        chunk: usize,
        resume: Option<Resume>,
        deliver: Option<usize>,
    ) -> usize {
        let (lines, language, _, _) = parts(doc);
        let (lines, language) = (lines.clone(), language.map(str::to_string));
        let source = Arc::clone(&doc.source);
        let mut chunks: Vec<Chunk> = Vec::new();
        spans_chunked(
            &source,
            &lines,
            language.as_deref(),
            chunk,
            std::time::Duration::MAX,
            resume.as_ref(),
            || true,
            |c| {
                chunks.push(c);
                deliver.is_none_or(|n| chunks.len() < n)
            },
        );
        let delivered = chunks.len();
        for c in chunks {
            let arrival = Arrival {
                block: 0,
                start_line: c.start_line,
                spans: c.spans,
                seam: Some(c.seam),
                converged: c.converged,
                reach: c.reach,
                speculative: false,
                slow: c.slow,
            };
            table.fold(doc, &arrival);
        }
        delivered
    }

    /// A key as the app applies it: the ledger's edit, the splice, then
    /// the table's walk from the frontier, with `alive` cutting the walk
    /// after as many lines as it answers true.
    fn key(
        doc: &mut Document,
        table: &mut Table,
        pos: usize,
        del: usize,
        insert: &str,
        alive: impl FnMut() -> bool,
    ) -> bool {
        let exact0 = parts(doc).3;
        let mut ledger = Ledger::new(Arc::clone(&doc.source), Vec::new());
        let touched = ledger.edit(pos..pos + del, insert);
        let current = ledger.current();
        let (old, new) = splice_document(doc, &current, pos..pos + del, touched)
            .expect("a code file takes the splice");
        let source = Arc::clone(&doc.source);
        let BlockKind::CodeBlock {
            lines,
            language,
            highlights,
            exact,
        } = &mut doc.blocks[0].kind
        else {
            unreachable!()
        };
        table.on_key(
            &source,
            lines,
            language.as_deref(),
            highlights,
            exact,
            exact0,
            &old,
            &new,
            Duration::MAX,
            alive,
        )
    }

    /// The byte offset where `line` starts.
    fn offset_of(doc: &Document, line: usize) -> usize {
        parts(doc)
            .0
            .line_range(line)
            .expect("a verbatim body")
            .start
    }

    /// Runs the resume of a block to its end and checks every row and
    /// every seam against a full parse.
    fn settle(doc: &mut Document, table: &mut Table, chunk: usize, what: &str) {
        let exact = parts(doc).3;
        let resume = table.resume(exact, &[]);
        sweep(doc, table, chunk, Some(resume), None);
        let want = full(doc);
        let (lines, _, highlights, exact) = parts(doc);
        assert_eq!(exact, lines.len(), "{what}: the frontier reaches the end");
        let wrong: Vec<usize> = (0..lines.len())
            .filter(|i| highlights[*i] != want[*i])
            .collect();
        assert!(
            wrong.is_empty(),
            "{what}: rows wrong after the resume: {wrong:?}"
        );
    }

    /// The read's recipe for claim 6 of the hunt of Task 328: a key that
    /// opens a comment, its resume killed by a key above before it
    /// reached the end, and the resume after that key. Before, the
    /// second resume converged on a seam the first sweep had refreshed
    /// and vouched up to the table's end, so the lines the first resume
    /// never reached kept their old colors with nothing owed.
    #[test]
    fn a_converged_stop_vouches_only_for_chained_seams() {
        for landed in [0, 1] {
            let text = let_lines(600);
            let mut doc = rust_doc(&text);
            let mut table = Table::default();
            sweep(&mut doc, &mut table, 64, None, None);
            assert_eq!(parts(&doc).3, 600);
            let pos = offset_of(&doc, 200);
            assert!(
                key(&mut doc, &mut table, pos, 0, "/*", || true),
                "a comment opened: owed"
            );
            let exact = parts(&doc).3;
            assert_eq!(exact, 201, "the walk's reach");
            let resume = table.resume(exact, &[200, 201]);
            sweep(&mut doc, &mut table, 64, Some(resume), Some(landed));
            let pos = offset_of(&doc, 150);
            assert!(key(&mut doc, &mut table, pos, 0, "x", || true));
            settle(&mut doc, &mut table, 64, &format!("{landed} chunks landed"));
        }
    }

    /// Claim 26: two keys handled before the resume between them, the
    /// second below the first with older seams between. Before, the
    /// walk of the second key moved the frontier over those seams
    /// without filing them again, and a later key walked from a stale
    /// one.
    #[test]
    fn the_walk_drops_the_seams_it_passes() {
        let text = let_lines(100);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 10, None, None);
        assert_eq!(lines_of(&table).len(), 10);
        let pos = offset_of(&doc, 30);
        assert!(key(&mut doc, &mut table, pos, 0, "/*", || true));
        let pos = offset_of(&doc, 45);
        assert!(key(&mut doc, &mut table, pos, 0, "x", || true));
        assert!(
            !table.seams().iter().any(|f| 31 < f.line && f.line < 45),
            "the seams the walk passed are gone: {:?}",
            lines_of(&table)
        );
        let pos = offset_of(&doc, 42);
        assert!(key(&mut doc, &mut table, pos, 0, "y", || true));
        settle(&mut doc, &mut table, 10, "three keys in one batch");
    }

    /// Claim 23: a block with no grammar is shifted and nothing else.
    #[test]
    fn a_block_without_a_grammar_is_never_walked() {
        let mut doc = load::text_document(&let_lines(50));
        let mut table = Table::default();
        let pos = offset_of(&doc, 20);
        assert!(
            !key(&mut doc, &mut table, pos, 0, "x", || true),
            "nothing owed"
        );
        assert!(table.is_empty(), "no seam filed");
        let (lines, _, highlights, exact) = parts(&doc);
        assert!(highlights.is_empty(), "no row colored");
        assert_eq!(exact, lines.len(), "the frontier at the line count");
    }

    /// Claim 27: a key on a line marked slow parses nothing on the
    /// window's thread; the worker colors the line, its chunk cut at the
    /// key's lines, and clears the mark when the parse is within the
    /// budget.
    #[test]
    fn a_key_on_a_slow_line_is_left_to_the_worker() {
        let text = let_lines(40);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 8, None, None);
        table.mark_slow(20);
        let before = parts(&doc).2.clone();
        let pos = offset_of(&doc, 20);
        assert!(
            key(&mut doc, &mut table, pos, 0, "x", || true),
            "owed to the worker"
        );
        let (_, _, highlights, exact) = parts(&doc);
        assert_eq!(exact, 20, "the frontier before the slow line");
        assert_eq!(highlights[21..], before[21..], "the rows below untouched");
        let resume = table.resume(exact, &[20, 21]);
        let delivered = sweep(&mut doc, &mut table, 8, Some(resume), Some(1));
        assert_eq!(delivered, 1);
        assert_eq!(
            parts(&doc).3,
            21,
            "the first chunk is the slow line alone, cut at the key's lines"
        );
        assert!(
            table.slow().is_empty(),
            "the worker's parse clears the mark"
        );
        settle(&mut doc, &mut table, 8, "a key on a slow line");
    }

    /// Rule 4 of Task 327: a walk that falls short leaves the typed text
    /// to the worker, and the resume colors it.
    #[test]
    fn a_walk_that_falls_short_leaves_the_typed_text_to_the_worker() {
        let text = let_lines(100);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 50, None, None);
        let pos = offset_of(&doc, 80);
        let mut budget = 3;
        let owed = key(&mut doc, &mut table, pos, 0, "/*", move || {
            budget -= 1;
            budget >= 0
        });
        assert!(owed);
        assert_eq!(
            parts(&doc).3,
            80,
            "three lines walked from the seam at 50; the lines before the edit stay verified"
        );
        assert_eq!(
            lines_of(&table),
            vec![(50, true), (53, false), (100, false)],
            "the walk's end unchained, the seam past the edit kept"
        );
        settle(&mut doc, &mut table, 50, "a walk that fell short");
    }

    /// A letter typed on a line of a colored block: the end state holds,
    /// the lines below stay verified, and the seam at the end of the
    /// key's line keeps its chain.
    #[test]
    fn a_key_that_leaves_the_end_state_keeps_the_lines_below_verified() {
        let text = let_lines(100);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 10, None, None);
        let pos = offset_of(&doc, 50);
        assert!(
            key(&mut doc, &mut table, pos, 0, "x", || true),
            "a chunk is still owed"
        );
        assert_eq!(
            parts(&doc).3,
            51,
            "no seam at the line's end: verified up to the walk"
        );
        settle(&mut doc, &mut table, 10, "a letter");
        let pos = offset_of(&doc, 50);
        assert!(
            !key(&mut doc, &mut table, pos, 0, "y", || true),
            "the end state is stored: nothing owed"
        );
        assert_eq!(parts(&doc).3, 100, "the lines below verified as before");
        assert!(
            table.seams().iter().any(|f| f.line == 51 && f.chained),
            "the seam at the line's end keeps its chain: {:?}",
            lines_of(&table)
        );
        assert_eq!(parts(&doc).2, &full(&doc), "every row right");
    }

    /// A caret seam lands between two seams with the flag of the one
    /// before it, and is not filed past the frontier.
    #[test]
    fn a_caret_seam_takes_the_flag_of_the_seam_before_it() {
        let text = let_lines(40);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 10, None, Some(2));
        assert_eq!(lines_of(&table), vec![(10, true), (20, false)]);
        let (lines, language, _, _) = parts(&doc);
        let caret = |line: usize| {
            let (start, seam) = table.start_at(line);
            let walked = walk(
                &doc.source,
                lines,
                language,
                start,
                seam,
                line,
                line,
                Duration::MAX,
                &[],
                || true,
            );
            Arrival {
                block: 0,
                start_line: line,
                spans: Vec::new(),
                seam: Some(walked.seam),
                converged: false,
                reach: 0,
                speculative: false,
                slow: Vec::new(),
            }
        };
        let (at_5, at_15, at_25) = (caret(5), caret(15), caret(25));
        table.fold(&mut doc, &at_5);
        table.fold(&mut doc, &at_15);
        table.fold(&mut doc, &at_25);
        assert_eq!(
            lines_of(&table),
            vec![(5, true), (10, true), (15, true), (20, false)],
            "inside a chained segment chained, past the frontier not filed"
        );
        let resume = table.resume(20, &[]);
        sweep(&mut doc, &mut table, 10, Some(resume), None);
        table.fold(&mut doc, &at_25);
        assert_eq!(
            lines_of(&table)[4],
            (25, true),
            "after the sweep, filed inside the chain: {:?}",
            lines_of(&table)
        );
        let pos = offset_of(&doc, 24);
        key(&mut doc, &mut table, pos, 0, "/*", || true);
        let resume = table.resume(parts(&doc).3, &[]);
        sweep(&mut doc, &mut table, 10, Some(resume), Some(1));
        table.shift(&(22..23), &(22..23));
        assert_eq!(
            lines_of(&table).iter().find(|f| f.0 == 20),
            Some(&(20, false)),
            "an edit unchains the seam before it: {:?}",
            lines_of(&table)
        );
    }

    #[test]
    fn the_table_stays_ordered_and_free_of_duplicates() {
        let text = let_lines(40);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 10, None, None);
        let seams: Vec<Filed> = table.seams().to_vec();
        let mut table = Table::default();
        for f in &seams {
            table.fold_chunk(f.line - 10, f.line, &f.seam, &[]);
        }
        assert_eq!(
            lines_of(&table),
            vec![(10, true), (20, true), (30, true), (40, false)],
            "every seam is kept, ordered by line, chained up to the last"
        );
        // Filed again out of order, as a resumed sweep's arrivals reach a
        // table the first sweep already seeded: the same states keep
        // their lines and their flags.
        for f in seams.iter().rev() {
            table.fold_chunk(f.line - 10, f.line, &f.seam, &[]);
        }
        assert_eq!(
            lines_of(&table),
            vec![(10, true), (20, true), (30, true), (40, false)],
            "a re-swept line updates in place"
        );
    }

    #[test]
    fn the_table_shifts_across_an_edit_and_unchains_the_seam_before_it() {
        let text = let_lines(1600);
        let mut doc = rust_doc(&text);
        let mut seeded = Table::default();
        sweep(&mut doc, &mut seeded, 512, None, None);
        seeded.mark_slow(700);
        seeded.mark_slow(1100);
        assert_eq!(
            lines_of(&seeded),
            vec![(512, true), (1024, true), (1536, true), (1600, false)]
        );
        let mut table = seeded.clone();
        table.shift(&(600..603), &(600..601));
        assert_eq!(
            lines_of(&table),
            vec![(512, false), (1022, true), (1534, true), (1598, false)],
            "entries before the edit stand, the nearest unchained, entries past it shift"
        );
        assert_eq!(
            table.slow(),
            &[698, 1098],
            "the slow marks shift the same way"
        );
        let mut table = seeded.clone();
        table.shift(&(1000..1030), &(1000..1030));
        assert_eq!(
            lines_of(&table),
            vec![(512, false), (1536, true), (1600, false)],
            "an entry inside the touched range drops"
        );
        let mut table = seeded.clone();
        table.shift(&(1000..1024), &(1000..1020));
        assert_eq!(
            lines_of(&table),
            vec![(512, false), (1020, true), (1532, true), (1596, false)],
            "an entry at the touched region's end moves with the rest"
        );
        let mut table = seeded.clone();
        table.shift(&(512..513), &(512..515));
        assert_eq!(
            lines_of(&table),
            vec![(512, false), (1026, true), (1538, true), (1602, false)],
            "an entry at the touched region's first line stands, unchained"
        );
        let mut table = seeded.clone();
        table.shift(&(100..101), &(100..102));
        assert!(
            !table.head,
            "an edit before the first seam unchains the fresh start"
        );
        assert!(seeded.head, "a sweep from line 0 chains the fresh start");
    }

    /// The resume starts at the seam at or before the frontier and
    /// converges against the seams past it; a guess waits while any
    /// seam stands past the frontier.
    #[test]
    fn a_resume_starts_before_the_frontier_and_converges_past_it() {
        let text = let_lines(3000);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 512, None, None);
        let shape = |resume: Resume| {
            (
                resume.start_line,
                resume.seam.is_some(),
                resume.expected.iter().map(|f| f.line).collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            shape(table.resume(100, &[])),
            (0, false, vec![512, 1024, 1536, 2048, 2560, 3000]),
            "no seam before the frontier: from line 0, every seam past it"
        );
        assert_eq!(
            shape(table.resume(600, &[600, 601])),
            (512, true, vec![1024, 1536, 2048, 2560, 3000]),
            "a frontier past a seam starts from that seam"
        );
        assert_eq!(table.resume(600, &[600, 601]).cuts, vec![600, 601]);
        assert_eq!(
            shape(table.resume(1024, &[])),
            (1024, true, vec![1536, 2048, 2560, 3000]),
            "a frontier on a seam starts from it and converges past it"
        );
        assert!(table.past_frontier(100));
        assert!(table.past_frontier(2999));
        assert!(
            !table.past_frontier(3000),
            "a complete table has nothing past the end"
        );
        assert!(
            !Table::default().past_frontier(0),
            "an empty table has nothing"
        );
    }

    #[test]
    fn the_frontier_after_a_key_follows_the_walk_and_the_end_state() {
        let after = frontier_after_key;
        assert_eq!(
            after(1000, &(600..601), &(600..601), 601, true),
            1000,
            "a colored block, a key on a line, the end state unchanged"
        );
        assert_eq!(
            after(1000, &(600..601), &(600..603), 603, true),
            1002,
            "two lines inserted: the frontier moves by the delta"
        );
        assert_eq!(
            after(1000, &(600..601), &(600..601), 601, false),
            601,
            "the end state changed: verified up to the walk"
        );
        assert_eq!(
            after(1000, &(600..601), &(600..601), 590, true),
            600,
            "the walk fell short: the lines before the edit stay verified"
        );
        assert_eq!(
            after(300, &(600..601), &(600..601), 340, true),
            340,
            "a burst open above: the walk's reach, no restoration"
        );
        assert_eq!(
            after(600, &(600..601), &(600..601), 601, true),
            601,
            "the frontier inside the touched lines before the key: no restoration"
        );
    }

    /// Where a caret seam is asked from: the nearest trusted seam when
    /// it is farther than the walk's reach from the caret's line, a
    /// fresh start when there is none, nothing when a seam is near or
    /// the caret is past the frontier.
    #[test]
    fn the_caret_seam_is_asked_only_far_from_a_trusted_seam() {
        let text = let_lines(20);
        let mut doc = rust_doc(&text);
        let mut table = Table::default();
        sweep(&mut doc, &mut table, 4, None, None);
        assert_eq!(
            table.seams().iter().map(|f| f.line).collect::<Vec<_>>(),
            [4, 8, 12, 16, 20]
        );
        let shape =
            |got: Option<(usize, Option<Seam>)>| got.map(|(line, seam)| (line, seam.is_some()));
        assert_eq!(
            shape(table.caret_ask(20, 9, 8)),
            None,
            "a seam one line above"
        );
        assert_eq!(shape(table.caret_ask(20, 8, 2)), None, "a seam at the line");
        assert_eq!(
            shape(table.caret_ask(20, 19, 2)),
            Some((16, true)),
            "three lines from the nearest seam, two of reach"
        );
        assert_eq!(
            shape(table.caret_ask(20, 3, 2)),
            Some((0, false)),
            "no seam before: a fresh start at line 0"
        );
        assert_eq!(
            shape(table.caret_ask(20, 3, 8)),
            None,
            "line 0 within reach"
        );
        assert_eq!(shape(table.caret_ask(10, 19, 2)), None, "past the frontier");
        assert_eq!(
            shape(Table::default().caret_ask(20, 19, 2)),
            Some((0, false)),
            "no table"
        );
    }
}

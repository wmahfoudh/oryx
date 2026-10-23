//! Maps syntect parse scopes onto theme syntax roles at load time.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

use crate::doc::model::CodeBody;

/// Styled ranges for one code line.
pub type LineSpans = Vec<(Range<usize>, SyntaxRole)>;

/// The byte length from which a source line counts as long: it is
/// colored plain without visiting the grammar, and the layout shapes
/// it in chunks of this size. A minified file puts hundreds of
/// kilobytes on one line, and cosmic-text's span list grows quadratic
/// in the number of styled pieces on a line: a 250KB JSON line with
/// 136,001 pieces took 106.8s to lay out, against 134ms plain.
pub const LONG_LINE: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxRole {
    Keyword,
    String,
    Number,
    Function,
    Type,
    Comment,
    Operator,
    Variable,
    Punctuation,
    Plain,
    /// A diff's lines: added, removed, and the `@@` line that says
    /// where a change sits. Only a grammar that marks text as inserted
    /// or deleted produces these, a diff above all.
    Added,
    Removed,
    Range,
    /// Markdown source roles. A markdown file edits its own bytes, so
    /// its source is colored from the document's theme, the heading
    /// ramp and the text and block colors, rather than from the code
    /// palette. Only the markdown grammar produces these.
    Heading(u8),
    Bold,
    Italic,
    InlineCode,
    Link,
    Quote,
    Rule,
}

/// The face a role asks for, as (bold, italic). A monospace family
/// draws both at the same advance width, so no glyph moves; every other
/// role keeps the regular face, since a source view's rows are a grid.
pub fn role_face(role: SyntaxRole) -> (bool, bool) {
    match role {
        // A heading line is bold in the source as on the page; bold
        // inside it keeps the heading's role, so the weight is the same.
        SyntaxRole::Bold | SyntaxRole::Heading(_) => (true, false),
        SyntaxRole::Italic => (false, true),
        _ => (false, false),
    }
}

/// Per-line styled ranges for a code block. Lines with no recognized
/// language come back as single Plain ranges.
pub fn spans(source: &str, lines: &CodeBody, language: Option<&str>) -> Vec<LineSpans> {
    spans_until(source, lines, language, None)
}

/// `spans` cut off at a deadline: only whole lines computed before the
/// deadline are returned, so the result is a prefix of the full output.
/// None means no deadline. The one-time grammar load happens before the
/// first deadline check and counts against the caller's budget.
pub fn spans_until(
    source: &str,
    lines: &CodeBody,
    language: Option<&str>,
    deadline: Option<Instant>,
) -> Vec<LineSpans> {
    let mut parser = Parser::new(language);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines.iter(source) {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        out.push(parser.line(line));
    }
    out
}

/// The parser state carried across a line boundary: equal seams parse
/// the rest of a block identically, which is what lets a sweep resume
/// mid-block and stop early.
#[derive(Debug, Clone, PartialEq)]
pub struct Seam {
    parse: ParseState,
    stack: ScopeStack,
}

/// Where a sweep starts when it does not sweep whole: the stored seam
/// for `start_line` (None starts fresh, meaningful only at line 0),
/// and the stored downstream table to converge against.
#[derive(Debug, Clone)]
pub struct Resume {
    pub start_line: usize,
    pub seam: Option<Seam>,
    pub expected: Vec<(usize, Seam)>,
    /// Lines where a chunk ends whatever the table says, so that a
    /// seam is filed there: the lines of the last key, where the next
    /// key's walk then starts.
    pub cuts: Vec<usize>,
}

/// Files a sweep's seam in the table, ordered by line: a line already
/// known updates in place, so a re-swept region never crowds the
/// table with a second entry for the same line. A seam holds about
/// 630 bytes, so one per 512-line chunk costs a third of a megabyte
/// on an 8MB source and the table stays dense enough that an edit
/// re-colors a single chunk.
pub fn record_seam(table: &mut Vec<(usize, Seam)>, line: usize, seam: &Seam) {
    match table.binary_search_by_key(&line, |(l, _)| *l) {
        Ok(at) => table[at].1 = seam.clone(),
        Err(at) => table.insert(at, (line, seam.clone())),
    }
}

/// Shifts a block's seam table across an edit that replaced the `old`
/// line range with `new`. Entries at or before the region's first line
/// stand, entries inside it describe replaced lines and drop, and
/// entries past its end move by the line delta. A shifted entry still
/// holds the pre-edit state for its line, which is what a resumed
/// sweep converges against.
pub fn shift_seams(
    table: &mut Vec<(usize, Seam)>,
    old: std::ops::Range<usize>,
    new: std::ops::Range<usize>,
) {
    let delta = new.end as isize - old.end as isize;
    table.retain_mut(|(line, _)| {
        if *line <= old.start {
            true
        } else if *line <= old.end {
            false
        } else {
            *line = (*line as isize + delta) as usize;
            true
        }
    });
}

/// Where a line stands after an edit replaced the `old` line range with
/// `new`, as `shift_seams` moves a seam: a line at or before the
/// region's first line stays, a line inside it is the region's new end,
/// a line past it moves by the line delta.
pub fn shift_line(line: usize, old: &Range<usize>, new: &Range<usize>) -> usize {
    if line <= old.start {
        line
    } else if line <= old.end {
        new.end
    } else {
        (line as isize + new.end as isize - old.end as isize) as usize
    }
}

/// Drops the seams between the frontier and the burst's end before a
/// resume: they describe lines an edit of the burst changed, and the
/// resumed sweep neither starts from them nor converges on them. The
/// seams at or before the frontier stand, and so do the ones past the
/// burst, which the sweep refreshes on its way.
pub fn drop_stale_seams(table: &mut Vec<(usize, Seam)>, exact: usize, past: usize) {
    table.retain(|(line, _)| *line <= exact || *line > past);
}

/// One delivered chunk: its lines' spans, the seam closing it, and
/// whether that seam matched the stored table, which stops the sweep
/// with the tail's colors and seams standing.
pub struct Chunk {
    pub start_line: usize,
    pub spans: Vec<LineSpans>,
    pub seam: Seam,
    pub converged: bool,
    /// The line a converged stop vouches up to: the stored table's
    /// reach, which the earlier sweep colored. Zero otherwise.
    pub reach: usize,
}

/// Highlights a block in chunks, handing each to `deliver`. Delivery
/// order is front to back; `deliver` returning false stops between
/// chunks. A resumed sweep starts from the stored seam and cuts its
/// chunks at the stored downstream lines, so each comparison lands
/// line-exact whatever line delta the table was shifted by; it stops
/// early after a chunk whose seam matches the table when the table
/// reaches the block's end, and goes on from the table's last seam when
/// it does not. A chunk also ends at the resume's cuts. `alive` is read
/// before each line: false stops the sweep there, the chunk in progress
/// undelivered, so a kill costs one line and not a chunk. Returns
/// whether the block completed, a converged stop included.
pub fn spans_chunked(
    source: &str,
    lines: &CodeBody,
    language: Option<&str>,
    chunk_size: usize,
    resume: Option<&Resume>,
    mut alive: impl FnMut() -> bool,
    mut deliver: impl FnMut(Chunk) -> bool,
) -> bool {
    let chunk_size = chunk_size.max(1);
    let mut parser = match resume.and_then(|r| r.seam.clone()) {
        Some(seam) => Parser::from_seam(seam, language),
        None => Parser::new(language),
    };
    let expected: &[(usize, Seam)] = resume.map_or(&[], |r| &r.expected);
    let cuts: &[usize] = resume.map_or(&[], |r| &r.cuts);
    let mut start = resume.map_or(0, |r| r.start_line);
    while start < lines.len() {
        let cap = start + chunk_size;
        let end = expected
            .iter()
            .map(|(line, _)| *line)
            .chain(cuts.iter().copied())
            .filter(|line| *line > start && *line < cap)
            .min()
            .unwrap_or(cap)
            .min(lines.len());
        let mut spans: Vec<LineSpans> = Vec::with_capacity(end - start);
        for i in start..end {
            if !alive() {
                return false;
            }
            spans.push(parser.line(lines.line(source, i)));
        }
        let seam = parser.seam();
        let converged = expected.iter().any(|(line, s)| *line == end && *s == seam);
        // A converged stop vouches for the lines up to the table's
        // reach, the last line the earlier sweep colored and filed, and
        // never for less than this chunk. A table that stops short of
        // the block's end, as a keystroke during the first sweep leaves
        // it, has a tail that sweep never reached: the sweep goes on
        // with it from the table's last seam when that seam is past
        // this chunk, and from this chunk's own seam when the table
        // ends here, so a table out of order ends the sweep as any
        // other does.
        let table_end = expected
            .last()
            .map_or(0, |(line, _)| *line)
            .min(lines.len());
        let reach = if converged { table_end.max(end) } else { 0 };
        if !deliver(Chunk {
            start_line: start,
            spans,
            seam,
            converged,
            reach,
        }) {
            return false;
        }
        if converged {
            if reach >= lines.len() {
                return true;
            }
            if reach > end {
                let (_, last_seam) = expected.last().expect("a converged stop has a table");
                parser = Parser::from_seam(last_seam.clone(), language);
                start = reach;
                continue;
            }
        }
        start = end;
    }
    true
}

/// What a walk on the window's thread delivers: the spans of the lines
/// it parsed, where it stopped, and the states it recorded on the way.
pub struct Walk {
    /// The first line parsed.
    pub start_line: usize,
    /// The spans of the lines from `start_line` to `reached`.
    pub spans: Vec<LineSpans>,
    /// The line the walk stopped before, the next one to parse.
    pub reached: usize,
    /// The state before `reached`.
    pub seam: Seam,
    /// The state before `mark`, when the walk passed that line.
    pub at_mark: Option<Seam>,
}

/// Parses the lines from `start_line`, continuing from `seam` (None
/// starts fresh, meaningful only at line 0), toward `until`, and stops
/// after the line on which `budget` runs out, one line at least: the
/// coloring of a key's lines on the window's thread, which the next
/// key continues from the seam it stopped at. Records the state before
/// `mark`, the first touched line, when the walk passes it.
#[allow(clippy::too_many_arguments)]
pub fn walk(
    source: &str,
    lines: &CodeBody,
    language: Option<&str>,
    start_line: usize,
    seam: Option<Seam>,
    mark: usize,
    until: usize,
    budget: std::time::Duration,
) -> Walk {
    let mut parser = match seam {
        Some(seam) => Parser::from_seam(seam, language),
        None => Parser::new(language),
    };
    let until = until.min(lines.len());
    let started = Instant::now();
    let mut at_mark = (mark == start_line).then(|| parser.seam());
    let mut spans = Vec::new();
    let mut line = start_line;
    while line < until {
        spans.push(parser.line(lines.line(source, line)));
        line += 1;
        if line == mark {
            at_mark = Some(parser.seam());
        }
        if line < until && started.elapsed() >= budget {
            break;
        }
    }
    Walk {
        start_line,
        spans,
        reached: line,
        seam: parser.seam(),
        at_mark,
    }
}

/// Lines per background delivery. Small enough that the top of a huge
/// block colors quickly, large enough that fold-ins stay rare.
pub const CHUNK_LINES: usize = 512;

/// Colors a band from the cold guess: a fresh parser advanced past one
/// empty line, which clears syntect's first-line flag and pushes the
/// base scope. The guess is right for over 99% of lines, wrong only
/// where the band opens inside a construct, so the result is
/// display-quality and the exact sweep still overwrites it.
pub fn spans_band(
    source: &str,
    lines: &CodeBody,
    language: Option<&str>,
    range: Range<usize>,
) -> Vec<LineSpans> {
    let start = range.start.min(lines.len());
    let end = range.end.min(lines.len());
    let mut parser = Parser::new(language);
    parser.line("");
    (start..end)
        .map(|i| parser.line(lines.line(source, i)))
        .collect()
}

/// One segment's report from `segment_probe`.
pub struct SegmentProbe {
    /// The truth state at the boundary equals the cold guess. Equal
    /// states parse the rest of the block identically, so a hit means
    /// a segment started from the guess delivers exact spans.
    pub state_hit: bool,
    /// Lines in the segment.
    pub lines: usize,
    /// Lines whose spans from the cold start differ from the truth.
    pub drifted_lines: usize,
}

/// Measurement probe for the segmented wash: walks the block in one
/// sequential truth pass and reports, for each `segment`-line boundary
/// past the first, whether the parser state carried into that line
/// equals the state a cold segment would guess, and how many of the
/// segment's lines a cold start would color differently. The guess is
/// a fresh parser advanced past one empty line, which clears syntect's
/// first-line flag and pushes the base scope; a raw fresh state
/// compares unequal at every boundary on those two artifacts alone.
pub fn segment_probe(
    source: &str,
    lines: &CodeBody,
    language: Option<&str>,
    segment: usize,
) -> Vec<SegmentProbe> {
    let segment = segment.max(1);
    let mut guess = Parser::new(language);
    guess.line("");
    let mut truth = Parser::new(language);
    let mut cold = None;
    let mut out: Vec<SegmentProbe> = Vec::new();
    for (index, line) in lines.iter(source).enumerate() {
        if index > 0 && index % segment == 0 {
            cold = Some(Parser {
                parse: guess.parse.clone(),
                stack: guess.stack.clone(),
                markdown: guess.markdown,
            });
            out.push(SegmentProbe {
                state_hit: truth.parse == guess.parse && truth.stack == guess.stack,
                lines: 0,
                drifted_lines: 0,
            });
        }
        let spans = truth.line(line);
        if let (Some(cold), Some(probe)) = (cold.as_mut(), out.last_mut()) {
            probe.lines += 1;
            if cold.line(line) != spans {
                probe.drifted_lines += 1;
            }
        }
    }
    out
}

/// A code block whose highlights were not finished inside the open
/// budget. The worker shares the source through the `Arc` and the line
/// ranges index it, so nothing owns a copy of the text.
pub struct PendingBlock {
    pub block: usize,
    pub language: Option<String>,
    pub source: Arc<str>,
    pub lines: CodeBody,
    /// A mid-block start with its seam and the stored table to
    /// converge against; None sweeps the block from line 0.
    pub resume: Option<Resume>,
}

/// One chunk of computed highlights for a block, `spans[i]` covering
/// line `start_line + i`.
pub struct Arrival {
    pub block: usize,
    pub start_line: usize,
    pub spans: Vec<LineSpans>,
    /// The state closing the chunk; None on a speculative arrival.
    pub seam: Option<Seam>,
    /// The chunk's seam matched the stored table: the colors and the
    /// seams up to `reach` stand.
    pub converged: bool,
    /// The line a converged chunk vouches up to; zero otherwise.
    pub reach: usize,
    /// Colored from a guessed state for display only; never advances
    /// the exact frontier and never feeds the seam table.
    pub speculative: bool,
}

/// A band of lines to color cold, ahead of the exact sweep, so a view
/// resting past the frontier does not read as plain text.
pub struct SpecJob {
    pub block: usize,
    pub language: Option<String>,
    pub source: Arc<str>,
    pub lines: CodeBody,
    pub range: Range<usize>,
}

/// A caret seam to compute ahead of the next key: the state before
/// `line`, parsed from `start_line` on from `seam` (None starts fresh
/// at line 0).
pub struct SeamJob {
    pub block: usize,
    pub language: Option<String>,
    pub source: Arc<str>,
    pub lines: CodeBody,
    pub start_line: usize,
    pub seam: Option<Seam>,
    pub line: usize,
}

/// Owns the background highlight worker and its arrivals queue. One
/// generation is live at a time; starting again cancels the old worker
/// at its next line and its arrivals are dropped at drain. Speculation
/// and the caret seam run beside it on generations of their own, and a
/// document swap cancels all three.
pub struct Highlighter {
    arrivals: Arc<Mutex<Vec<(u64, Arrival)>>>,
    generation: Arc<AtomicU64>,
    /// Highest generation that ran to completion. A cancelled worker
    /// never raises it, so `is_running` tracks the live generation
    /// alone. An export waits on it, since a PDF cannot wash in after
    /// it is written.
    done: Arc<AtomicU64>,
    /// The live speculation's generation; a new band or a document
    /// swap bumps it and the old one's result is discarded.
    spec: Arc<AtomicU64>,
    /// The live caret seam's generation, bumped the same way.
    caret: Arc<AtomicU64>,
}

impl Default for Highlighter {
    fn default() -> Highlighter {
        Highlighter::new()
    }
}

impl Highlighter {
    pub fn new() -> Highlighter {
        Highlighter {
            arrivals: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
            done: Arc::new(AtomicU64::new(0)),
            spec: Arc::new(AtomicU64::new(0)),
            caret: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Computes a caret seam off the window's thread: the arrival
    /// carries the state before `job.line` and no spans, the sign the
    /// fold reads it by, and lands in the queue for the next drain. A
    /// new ask or a document swap discards a result still in flight.
    pub fn seam_at(&mut self, job: SeamJob, waker: impl Fn() + Send + 'static) {
        let ask = self.caret.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.load(Ordering::SeqCst);
        let live_ask = Arc::clone(&self.caret);
        let live = Arc::clone(&self.generation);
        let arrivals = Arc::clone(&self.arrivals);
        std::thread::spawn(move || {
            let walked = walk(
                &job.source,
                &job.lines,
                job.language.as_deref(),
                job.start_line,
                job.seam,
                job.line,
                job.line,
                std::time::Duration::MAX,
            );
            if live_ask.load(Ordering::SeqCst) != ask || live.load(Ordering::SeqCst) != generation {
                return;
            }
            arrivals.lock().expect("arrivals lock").push((
                generation,
                Arrival {
                    block: job.block,
                    start_line: job.line,
                    spans: Vec::new(),
                    seam: Some(walked.seam),
                    converged: false,
                    reach: 0,
                    speculative: false,
                },
            ));
            waker();
        });
    }

    /// Colors one band from the cold guess state, off the exact
    /// sweep's path: the arrival is tagged speculative, carries no
    /// seam, and lands in the same queue for the next drain. A new
    /// band or a document swap discards a result still in flight.
    pub fn speculate(&mut self, job: SpecJob, waker: impl Fn() + Send + 'static) {
        let spec = self.spec.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.load(Ordering::SeqCst);
        let live_spec = Arc::clone(&self.spec);
        let live = Arc::clone(&self.generation);
        let arrivals = Arc::clone(&self.arrivals);
        std::thread::spawn(move || {
            let start = job.range.start.min(job.lines.len());
            let spans = spans_band(
                &job.source,
                &job.lines,
                job.language.as_deref(),
                job.range.clone(),
            );
            if live_spec.load(Ordering::SeqCst) != spec || live.load(Ordering::SeqCst) != generation
            {
                return;
            }
            arrivals.lock().expect("arrivals lock").push((
                generation,
                Arrival {
                    block: job.block,
                    start_line: start,
                    spans,
                    seam: None,
                    converged: false,
                    reach: 0,
                    speculative: true,
                },
            ));
            waker();
        });
    }

    /// Cancels any running worker, then highlights `pending` front to
    /// back on a fresh thread; each chunk lands in the arrivals queue
    /// and `waker` runs after it so the event loop can drain.
    pub fn start(&mut self, pending: Vec<PendingBlock>, waker: impl Fn() + Send + 'static) {
        // Speculation and the caret seam describe the text the old
        // generation held; a start means that text moved.
        self.spec.fetch_add(1, Ordering::SeqCst);
        self.caret.fetch_add(1, Ordering::SeqCst);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if pending.is_empty() {
            self.done.fetch_max(generation, Ordering::SeqCst);
            return;
        }
        let arrivals = Arc::clone(&self.arrivals);
        let current = Arc::clone(&self.generation);
        let done = Arc::clone(&self.done);
        std::thread::spawn(move || {
            for p in pending {
                let delivered = spans_chunked(
                    &p.source,
                    &p.lines,
                    p.language.as_deref(),
                    CHUNK_LINES,
                    p.resume.as_ref(),
                    || current.load(Ordering::SeqCst) == generation,
                    |chunk| {
                        if current.load(Ordering::SeqCst) != generation {
                            return false;
                        }
                        let arrival = Arrival {
                            block: p.block,
                            start_line: chunk.start_line,
                            spans: chunk.spans,
                            seam: Some(chunk.seam),
                            converged: chunk.converged,
                            reach: chunk.reach,
                            speculative: false,
                        };
                        arrivals
                            .lock()
                            .expect("arrivals lock")
                            .push((generation, arrival));
                        waker();
                        true
                    },
                );
                if !delivered {
                    return;
                }
            }
            done.fetch_max(generation, Ordering::SeqCst);
        });
    }

    /// Whether the live worker still has blocks to color.
    pub fn is_running(&self) -> bool {
        self.done.load(Ordering::SeqCst) < self.generation.load(Ordering::SeqCst)
    }

    /// Arrivals of the current generation in delivery order; stale
    /// generations are dropped.
    pub fn drain(&mut self) -> Vec<Arrival> {
        let generation = self.generation.load(Ordering::SeqCst);
        self.arrivals
            .lock()
            .expect("arrivals lock")
            .drain(..)
            .filter(|(g, _)| *g == generation)
            .map(|(_, a)| a)
            .collect()
    }
}

/// Sequential syntect state over one code block; lines must be fed in
/// order from the block's first line.
struct Parser {
    parse: ParseState,
    stack: ScopeStack,
    /// The markdown grammar resolves to the document's own colors
    /// rather than the code palette. Fixed at construction, so a code
    /// file can never reach a markdown role.
    markdown: bool,
}

impl Parser {
    fn new(language: Option<&str>) -> Parser {
        let syntax = language
            .and_then(resolve_syntax)
            .unwrap_or_else(|| syntax_set().find_syntax_plain_text());
        Parser {
            parse: ParseState::new(syntax),
            stack: ScopeStack::new(),
            markdown: is_markdown(language),
        }
    }

    /// A parser continuing from a recorded seam; the state embeds its
    /// contexts, so no language lookup is involved.
    fn from_seam(seam: Seam, language: Option<&str>) -> Parser {
        Parser {
            parse: seam.parse,
            stack: seam.stack,
            markdown: is_markdown(language),
        }
    }

    fn seam(&self) -> Seam {
        Seam {
            parse: self.parse.clone(),
            stack: self.stack.clone(),
        }
    }

    fn line(&mut self, line: &str) -> LineSpans {
        // A long line never reaches the grammar; the parser state stays
        // where the previous line left it, so a sweep resumed over the
        // block still converges.
        if line.len() >= LONG_LINE {
            return vec![(0..line.len(), SyntaxRole::Plain)];
        }
        // The grammar levels only the first two headings, so the rest
        // are read off the line itself.
        let hashes = if self.markdown {
            line.bytes().take_while(|b| *b == b'#').count().min(6) as u8
        } else {
            0
        };
        let text = format!("{line}\n");
        let ops = self
            .parse
            .parse_line(&text, syntax_set())
            .unwrap_or_default();
        let mut tokens: Vec<Token> = Vec::new();
        let mut last = 0usize;
        let markdown = self.markdown;
        // A diff's line is added, removed or a range whole, its scope
        // pushed at the line's start: the first token answers for the
        // line, and the other tokens never look.
        let mut line_role: Option<Option<SyntaxRole>> = None;
        let mut push = |from: usize, to: usize, stack: &ScopeStack| {
            let to = to.min(line.len());
            if from < to {
                let role = if markdown {
                    markdown_role(stack, hashes)
                } else {
                    let diff = *line_role.get_or_insert_with(|| diff_role(stack));
                    diff.unwrap_or_else(|| role_for(stack))
                };
                tokens.push(Token {
                    role,
                    link: markdown
                        && (stack_has(stack, "meta.link") || stack_has(stack, "meta.image")),
                    closes_text: markdown
                        && (stack_has(stack, "punctuation.definition.link.end")
                            || stack_has(stack, "punctuation.definition.image.end")),
                    blank: line[from..to].trim().is_empty(),
                    illegal: markdown && stack_has(stack, "invalid.illegal.whitespace"),
                    range: from..to,
                });
            }
        };
        for (index, op) in &ops {
            push(last, *index, &self.stack);
            last = (*index).max(last);
            let _ = self.stack.apply(op);
        }
        push(last, line.len(), &self.stack);
        if markdown {
            demote_broken_links(&mut tokens);
        }
        let mut ranges: LineSpans = Vec::new();
        for token in tokens {
            match ranges.last_mut() {
                Some((prev, r)) if *r == token.role && prev.end == token.range.start => {
                    prev.end = token.range.end;
                }
                _ => ranges.push((token.range, token.role)),
            }
        }
        ranges
    }
}

/// One grammar token of a line before adjacent equal roles merge.
struct Token {
    range: Range<usize>,
    role: SyntaxRole,
    /// Inside a `meta.link` or `meta.image` construct of the markdown
    /// grammar.
    link: bool,
    /// The `]` closing the link or image text.
    closes_text: bool,
    /// Whitespace only.
    blank: bool,
    /// The grammar's `invalid.illegal.whitespace` tag on the token.
    illegal: bool,
}

/// Whether a scope of `stack` starts with `prefix`.
fn stack_has(stack: &ScopeStack, prefix: &str) -> bool {
    Scope::new(prefix).is_ok_and(|wanted| stack.as_slice().iter().any(|s| wanted.is_prefix_of(*s)))
}

/// The markdown grammar still reads `[text] (url)`, `[text] [ref]` and
/// `![alt] (file)`, a space after the closing bracket of the text, as
/// links and images, the way the first Markdown allowed; CommonMark and
/// the reader see plain text there, and a task box before a parenthesis
/// (`- [ ] (name) ...`) is a common shape. A construct whose text
/// bracket is followed by whitespace (the grammar tags some of them
/// `invalid.illegal.whitespace`, the reference form not) becomes plain
/// text as a whole. The spaces a link may hold, before a title or after
/// a definition's colon, follow other tokens and stay.
fn demote_broken_links(tokens: &mut [Token]) {
    let mut start = 0;
    while start < tokens.len() {
        if !tokens[start].link {
            start += 1;
            continue;
        }
        let end = tokens[start..]
            .iter()
            .position(|t| !t.link)
            .map_or(tokens.len(), |n| start + n);
        let broken = (start..end).any(|i| {
            tokens[i].illegal || (tokens[i].blank && i > start && tokens[i - 1].closes_text)
        });
        if broken {
            for token in &mut tokens[start..end] {
                if token.role == SyntaxRole::Link {
                    token.role = SyntaxRole::Plain;
                }
            }
        }
        start = end;
    }
}

/// The default set plus the grammars bundled from `assets/syntaxes/`,
/// compiled into one dump by `build.rs` and embedded here.
fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        syntect::dumps::from_binary(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/syntaxes.packdump"
        )))
    })
}

/// Tokens the set does not answer directly. A lookup matches a syntax
/// name or one of its extensions, so a syntax whose name carries
/// punctuation is reachable only through an extension: that is `batch`
/// and `graphviz`, named "Batch File" and "Graphviz (DOT)". The rest
/// bridge a language token to a grammar shipped under another name:
/// Docker's grammar is named Containerfile and lists only capitalized
/// file names, and HCL's grammar ships under the Terraform product name.
const ALIASES: &[(&str, &str)] = &[
    ("batch", "bat"),
    ("csharp", "cs"),
    ("docker", "containerfile"),
    ("dockerfile", "containerfile"),
    ("graphviz", "dot"),
    ("hcl", "terraform"),
];

/// The bundled grammar a language token resolves to, by its syntax name,
/// or `None` when nothing answers it and the text renders unstyled.
pub fn grammar_of(token: &str) -> Option<&'static str> {
    resolve_syntax(token).map(|s| s.name.as_str())
}

fn resolve_syntax(token: &str) -> Option<&'static syntect::parsing::SyntaxReference> {
    let set = syntax_set();
    set.find_syntax_by_token(token).or_else(|| {
        ALIASES
            .iter()
            .find(|(from, _)| *from == token)
            .and_then(|(_, to)| set.find_syntax_by_token(to))
    })
}

/// How much of a first line the first-line rules read, in bytes.
const FIRST_LINE_READ: usize = 1024;

/// The grammar for a file whose name carries no extension, from what
/// the file says about itself, in four layers tried in order: its first
/// line against the grammars' own first-line patterns (shebangs, an XML
/// header), an editor modeline in its first or last lines, its name
/// against the grammars' own name lists (`.bashrc`, `Gemfile`), and the
/// shape of its first lines for four plain tells only, a diff, JSON,
/// XML or INI, so a settings file is never colored as the wrong
/// language. None keeps the file plain; a wrong guess costs colors,
/// never the text. Answers the grammar's name, which resolves as a
/// token.
pub fn sniff_language(name: &str, text: &str) -> Option<&'static str> {
    let set = syntax_set();
    // The first-line rules read a shebang or a header. A file with no
    // line break is one line of any length, and the rules run over all
    // they are given (105 ms at 8 MB), so they get its head alone.
    let first = text.lines().next().unwrap_or("");
    let mut head = first.len().min(FIRST_LINE_READ);
    while !first.is_char_boundary(head) {
        head -= 1;
    }
    if let Some(syntax) = set.find_syntax_by_first_line(&first[..head]) {
        return Some(syntax.name.as_str());
    }
    if let Some(syntax) = modeline_token(text).and_then(|token| resolve_syntax(&token)) {
        return Some(syntax.name.as_str());
    }
    if let Some(syntax) = set.find_syntax_by_extension(name) {
        return Some(syntax.name.as_str());
    }
    shape_token(text)
        .and_then(resolve_syntax)
        .map(|syntax| syntax.name.as_str())
}

/// The file extension of the grammar a text names by itself, `diff` for
/// a diff, `sh` for a shell script: the name a text with no file behind
/// it is given, so the ordinary open path colors it. None for a text
/// that says nothing.
pub fn sniff_extension(text: &str) -> Option<&'static str> {
    let name = sniff_language("", text)?;
    syntax_set()
        .find_syntax_by_name(name)?
        .file_extensions
        .first()
        .map(String::as_str)
}

/// The language a modeline names, in the vim form (`vim: set ft=sh:`,
/// `vi:ft=sh`, `syntax=sh`) or the Emacs form (`-*- mode: python -*-`,
/// `-*- python -*-`), looked for in the first and the last five lines.
fn modeline_token(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let edge = lines.len().min(5);
    let candidates = lines[..edge]
        .iter()
        .chain(lines[lines.len() - edge..].iter());
    // A modeline is a short comment; a line longer than that is data,
    // and a search through megabytes of it names nothing.
    for line in candidates.filter(|line| line.len() <= FIRST_LINE_READ) {
        if let Some(token) = vim_modeline(line).or_else(|| emacs_modeline(line)) {
            return Some(token);
        }
    }
    None
}

fn vim_modeline(line: &str) -> Option<String> {
    let start = ["vim:", "vi:", "ex:"]
        .iter()
        .filter_map(|marker| {
            line.find(marker)
                .filter(|&i| i == 0 || !line.as_bytes()[i - 1].is_ascii_alphanumeric())
                .map(|i| i + marker.len())
        })
        .min()?;
    line[start..]
        .split(|c: char| c.is_whitespace() || c == ':')
        .find_map(|word| {
            let value = word
                .strip_prefix("filetype=")
                .or_else(|| word.strip_prefix("ft="))
                .or_else(|| word.strip_prefix("syntax="))?;
            (!value.is_empty()).then(|| value.to_ascii_lowercase())
        })
}

fn emacs_modeline(line: &str) -> Option<String> {
    let open = line.find("-*-")? + 3;
    let close = line[open..].find("-*-")? + open;
    let inner = line[open..close].trim();
    let token = match inner.find("mode:") {
        Some(i) => inner[i + 5..]
            .trim_start()
            .split(|c: char| c == ';' || c.is_whitespace())
            .next()?,
        None if !inner.contains(':') => inner,
        None => return None,
    };
    (!token.is_empty()).then(|| token.to_ascii_lowercase())
}

/// The four plain shapes told from the first lines: a diff, a JSON
/// object or array with a quoted key, XML or HTML by its tags, INI by
/// a section line over a key line. Anything else says nothing.
fn shape_token(text: &str) -> Option<&'static str> {
    let head: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(20)
        .collect();
    let first = head.first()?.trim_start();
    let is_diff = first.starts_with("diff --git")
        || first.starts_with("Index: ")
        || (first.starts_with("--- ") && head.iter().any(|line| line.starts_with("+++ ")))
        || head
            .iter()
            .any(|line| line.starts_with("@@ ") && line.contains(" @@"));
    if is_diff {
        return Some("diff");
    }
    if (first.starts_with('{') || first.starts_with('['))
        && head
            .iter()
            .any(|line| line.contains("\":") || line.contains("\" :"))
    {
        return Some("json");
    }
    let tag_open = first.starts_with('<')
        && first[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '?' || c == '!');
    if tag_open && (text.contains("</") || text.contains("/>")) {
        return Some("xml");
    }
    let section =
        first.starts_with('[') && first.trim_end().ends_with(']') && !first.contains("](");
    if section && head.get(1).is_some_and(|line| line.contains('=')) {
        return Some("ini");
    }
    None
}

/// Whether a language token resolves to the markdown grammar, which is
/// the only one whose scopes carry the document's own colors.
fn is_markdown(language: Option<&str>) -> bool {
    language
        .and_then(resolve_syntax)
        .is_some_and(|s| s.name == "Markdown")
}

/// The markdown source roles, resolved from the outside in so a marker
/// takes the color of the construct holding it: the grammar scopes `**`
/// as `punctuation.definition.bold` nested inside `markup.bold`, and
/// the reader means the bold run. `hashes` is the line's leading run of
/// `#`, since the grammar levels only the first two headings.
fn markdown_role(stack: &ScopeStack, hashes: u8) -> SyntaxRole {
    for scope in stack.as_slice() {
        let name = scope.build_string();
        let role = if let Some(rest) = name.strip_prefix("markup.heading") {
            let level = rest
                .strip_prefix('.')
                .and_then(|r| r.split('.').next())
                .and_then(|d| d.parse::<u8>().ok())
                .unwrap_or(hashes)
                .clamp(1, 6);
            SyntaxRole::Heading(level)
        } else if name.starts_with("markup.bold") {
            SyntaxRole::Bold
        } else if name.starts_with("markup.italic") {
            SyntaxRole::Italic
        } else if name.starts_with("markup.raw") {
            SyntaxRole::InlineCode
        } else if name.starts_with("meta.link") || name.starts_with("markup.underline.link") {
            SyntaxRole::Link
        } else if name.starts_with("punctuation.definition.blockquote") {
            SyntaxRole::Quote
        } else if name.starts_with("markup.quote") {
            // The quoted text reads as body text, as it renders; only
            // the `>` wears the role, and marks inside the quote keep
            // their own.
            continue;
        } else if name.starts_with("meta.separator") {
            SyntaxRole::Rule
        } else {
            continue;
        };
        return role;
    }
    SyntaxRole::Plain
}

/// The whole-line role of a diff's line, from its first token's stack:
/// a line marked as inserted, deleted or as a range is that from its
/// first character to its last, its `+`, `-` or `@@` included, since a
/// diff is read by whole lines of one color. The scopes are compared
/// as scopes, outermost first, and asked once per line: as a string
/// per scope per token it was a second allocation pass over every
/// colored line, 14% on the 8 MB code highlight at Gate 3.
fn diff_role(stack: &ScopeStack) -> Option<SyntaxRole> {
    static DIFF_ROLES: OnceLock<[(Scope, SyntaxRole); 3]> = OnceLock::new();
    let diff_roles = DIFF_ROLES.get_or_init(|| {
        let scope = |name: &str| Scope::new(name).expect("a fixed scope name parses");
        [
            (scope("markup.inserted"), SyntaxRole::Added),
            (scope("markup.deleted"), SyntaxRole::Removed),
            (scope("meta.diff.range"), SyntaxRole::Range),
        ]
    });
    for scope in stack.as_slice() {
        for (prefix, role) in diff_roles {
            if prefix.is_prefix_of(*scope) {
                return Some(*role);
            }
        }
    }
    None
}

/// The innermost scope with a known mapping wins.
fn role_for(stack: &ScopeStack) -> SyntaxRole {
    for scope in stack.as_slice().iter().rev() {
        let name = scope.build_string();
        let role = if name.starts_with("comment") {
            SyntaxRole::Comment
        } else if name.starts_with("string") {
            SyntaxRole::String
        } else if name.starts_with("constant.numeric") {
            SyntaxRole::Number
        } else if name.starts_with("constant") {
            SyntaxRole::Keyword
        } else if name.starts_with("keyword.operator") {
            SyntaxRole::Operator
        } else if name.starts_with("keyword") || name.starts_with("storage") {
            SyntaxRole::Keyword
        } else if name.starts_with("entity.name.function")
            || name.starts_with("support.function")
            || name.starts_with("variable.function")
        {
            SyntaxRole::Function
        } else if name.starts_with("entity.name") || name.starts_with("support") {
            SyntaxRole::Type
        } else if name.starts_with("punctuation") {
            SyntaxRole::Punctuation
        } else if name.starts_with("variable") {
            SyntaxRole::Variable
        } else {
            continue;
        };
        return role;
    }
    SyntaxRole::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_tells_its_added_removed_and_range_lines() {
        let text = "diff --git a/notes.md b/notes.md\nindex 1d7918d..2323a24 100644\n--- a/notes.md\n+++ b/notes.md\n@@ -1,4 +1,5 @@\n # Notes\n-An old line.\n+A new line.\n";
        let body = CodeBody::from_text(text);
        let got = spans(text, &body, Some("diff"));
        let roles =
            |line: usize| -> Vec<SyntaxRole> { got[line].iter().map(|(_, role)| *role).collect() };
        assert_eq!(
            roles(6),
            vec![SyntaxRole::Removed],
            "the whole line, its minus included"
        );
        assert_eq!(got[6][0].0, 0..13);
        assert_eq!(roles(7), vec![SyntaxRole::Added]);
        assert_eq!(roles(4), vec![SyntaxRole::Range], "the @@ line");
        assert!(
            !roles(5).contains(&SyntaxRole::Added) && !roles(5).contains(&SyntaxRole::Removed),
            "a context line keeps the text color"
        );
        assert!(
            !roles(2).contains(&SyntaxRole::Removed) && !roles(3).contains(&SyntaxRole::Added),
            "the two file lines are names, not changes"
        );
    }

    fn lines(v: &[&str]) -> CodeBody {
        CodeBody::from_text(&v.join("\n"))
    }

    fn role_at(line: &[(Range<usize>, SyntaxRole)], pos: usize) -> SyntaxRole {
        line.iter()
            .find(|(r, _)| r.contains(&pos))
            .map(|(_, role)| *role)
            .unwrap_or(SyntaxRole::Plain)
    }

    fn pending_blocks(count: usize, lines_each: usize) -> Vec<PendingBlock> {
        (0..count)
            .map(|block| PendingBlock {
                block,
                source: Arc::from(""),
                language: None,
                lines: CodeBody::from_text(&"plain text line\n".repeat(lines_each)),
                resume: None,
            })
            .collect()
    }

    #[test]
    fn an_empty_start_reports_not_running() {
        let mut h = Highlighter::new();
        h.start(pending_blocks(8, 600), || {});
        assert!(h.is_running());
        h.start(Vec::new(), || {});
        assert!(!h.is_running(), "an empty start supersedes the old worker");
    }

    // The block counts sweep the cancellation point across chunk
    // boundaries; the failure is the flag dropping while the live
    // worker still has chunks to deliver.
    #[test]
    fn a_cancelled_worker_does_not_mark_the_next_one_done() {
        for blocks in [4usize, 16, 64] {
            let mut h = Highlighter::new();
            let second = pending_blocks(blocks, 600);
            let expected: usize = second
                .iter()
                .map(|p| p.lines.len().div_ceil(CHUNK_LINES))
                .sum();
            h.start(pending_blocks(blocks, 600), || {});
            h.start(second, || {});
            let mut got = 0usize;
            for _ in 0..20_000 {
                got += h.drain().len();
                if got >= expected || !h.is_running() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            got += h.drain().len();
            assert_eq!(
                got, expected,
                "{blocks} blocks: the flag dropped before the live worker finished"
            );
        }
    }

    #[test]
    fn rust_keyword_string_comment() {
        let src = lines(&["fn main() {", "    // a note", "    let s = \"hi\";", "}"]);
        let h = spans("", &src, Some("rust"));
        assert_eq!(h.len(), 4);
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "fn");
        assert_eq!(role_at(&h[1], 6), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[2], 13), SyntaxRole::String, "string literal");
    }

    #[test]
    fn python_keyword() {
        let src = lines(&["def greet(name):", "    return name"]);
        let h = spans("", &src, Some("python"));
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "def");
        assert_eq!(role_at(&h[1], 4), SyntaxRole::Keyword, "return");
    }

    /// Every byte of `text` inside the line resolves to the same role,
    /// delimiters included, which is what makes a marker take the color
    /// of the construct it belongs to.
    fn whole_construct(line: &str, text: &str, want: SyntaxRole) {
        let src = lines(&[line]);
        let h = spans("", &src, Some("md"));
        let at = line.find(text).expect("the construct is in the line");
        for pos in at..at + text.len() {
            assert_eq!(
                role_at(&h[0], pos),
                want,
                "{line:?} byte {pos} ({:?}) inside {text:?}",
                &line[pos..pos + 1]
            );
        }
    }

    #[test]
    fn markdown_markers_wear_their_construct_color() {
        whole_construct("Text **bold** here.", "**bold**", SyntaxRole::Bold);
        whole_construct("Text *slanted* here.", "*slanted*", SyntaxRole::Italic);
        whole_construct("Text `code` here.", "`code`", SyntaxRole::InlineCode);
        whole_construct("# Title", "# Title", SyntaxRole::Heading(1));
        whole_construct("> quoted line", ">", SyntaxRole::Quote);
        whole_construct("> quoted line", "quoted line", SyntaxRole::Plain);
        let nested = spans("", &lines(&["> > deeper"]), Some("md"));
        for pos in [0, 2] {
            assert_eq!(
                role_at(&nested[0], pos),
                SyntaxRole::Quote,
                "nested marker at {pos}"
            );
        }
        assert_eq!(role_at(&nested[0], 4), SyntaxRole::Plain, "nested text");
        whole_construct("> **bold** here", "**bold**", SyntaxRole::Bold);
        whole_construct("---", "---", SyntaxRole::Rule);
        whole_construct(
            "A [label](https://example.com) here.",
            "[label](https://example.com)",
            SyntaxRole::Link,
        );
    }

    #[test]
    fn heading_levels_follow_the_hashes() {
        for level in 1..=6u8 {
            let line = format!("{} Title", "#".repeat(level as usize));
            whole_construct(&line, &line, SyntaxRole::Heading(level));
        }
    }

    #[test]
    fn a_fence_is_one_construct_from_marker_to_marker() {
        let src = lines(&["```rust", "let x = 1;", "```"]);
        let h = spans("", &src, Some("md"));
        for (row, line) in ["```rust", "let x = 1;", "```"].iter().enumerate() {
            for pos in 0..line.len() {
                assert_eq!(
                    role_at(&h[row], pos),
                    SyntaxRole::InlineCode,
                    "fence row {row} byte {pos}"
                );
            }
        }
    }

    #[test]
    fn a_code_file_never_meets_a_markdown_role() {
        let src = lines(&["fn main() {", "    // a note", "    let s = \"hi\";", "}"]);
        let h = spans("", &src, Some("rust"));
        for line in &h {
            for (range, role) in line {
                assert!(
                    !matches!(
                        role,
                        SyntaxRole::Heading(_)
                            | SyntaxRole::Bold
                            | SyntaxRole::Italic
                            | SyntaxRole::InlineCode
                            | SyntaxRole::Link
                            | SyntaxRole::Quote
                            | SyntaxRole::Rule
                    ),
                    "{range:?} took {role:?}, a markdown role, in a rust file"
                );
            }
        }
    }

    /// Headings draw bold in the source as they do on the page, every
    /// level alike; a monospace bold keeps the advance, so the grid holds.
    #[test]
    fn bold_italic_and_headings_change_the_face() {
        assert_eq!(role_face(SyntaxRole::Bold), (true, false));
        assert_eq!(role_face(SyntaxRole::Italic), (false, true));
        for level in 1..=6 {
            assert_eq!(
                role_face(SyntaxRole::Heading(level)),
                (true, false),
                "heading {level} is bold"
            );
        }
        for role in [
            SyntaxRole::InlineCode,
            SyntaxRole::Link,
            SyntaxRole::Quote,
            SyntaxRole::Rule,
            SyntaxRole::Plain,
            SyntaxRole::Keyword,
        ] {
            assert_eq!(role_face(role), (false, false), "{role:?} keeps the face");
        }
    }

    #[test]
    fn tokens_reach_their_grammar_directly_or_by_alias() {
        assert_eq!(grammar_of("rust"), Some("Rust"));
        assert_eq!(grammar_of("haskell"), Some("Haskell"));
        assert_eq!(grammar_of("batch"), Some("Batch File"));
        assert_eq!(grammar_of("graphviz"), Some("Graphviz (DOT)"));
        assert_eq!(grammar_of("csharp"), Some("C#"));
        assert_eq!(grammar_of("typescript"), Some("TypeScript"));
        assert_eq!(grammar_of("ts"), Some("TypeScript"));
        assert_eq!(grammar_of("tsx"), Some("TypeScriptReact"));
        assert_eq!(grammar_of("toml"), Some("TOML"));
        assert_eq!(grammar_of("ini"), Some("INI"));
        assert_eq!(grammar_of("kotlin"), Some("Kotlin"));
        assert_eq!(grammar_of("swift"), Some("Swift"));
        assert_eq!(grammar_of("zig"), Some("Zig"));
        assert_eq!(grammar_of("dockerfile"), Some("Containerfile"));
        assert_eq!(grammar_of("docker"), Some("Containerfile"));
        assert_eq!(grammar_of("terraform"), Some("Terraform"));
        assert_eq!(grammar_of("hcl"), Some("Terraform"));
        assert_eq!(grammar_of("graphql"), Some("GraphQL"));
        assert_eq!(grammar_of("protobuf"), Some("Protocol Buffer"));
        assert_eq!(grammar_of("proto"), Some("Protocol Buffer"));
    }

    #[test]
    fn a_language_with_no_grammar_and_no_alias_resolves_to_nothing() {
        assert_eq!(grammar_of("nosuchlang"), None);
    }

    #[test]
    fn toml_comments_and_values_carry_their_roles() {
        let src = lines(&[
            "# build profile",
            "version = \"0.7.0\"",
            "lto = true",
            "opt-level = 3",
        ]);
        let h = spans("", &src, Some("toml"));
        assert_eq!(role_at(&h[0], 3), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[1], 12), SyntaxRole::String, "quoted value");
        assert_eq!(role_at(&h[2], 6), SyntaxRole::Keyword, "boolean");
        assert_eq!(role_at(&h[3], 12), SyntaxRole::Number, "number");
    }

    #[test]
    fn ini_keys_and_hash_comments_carry_their_roles() {
        let src = lines(&["# a note", "Fullscreen=true"]);
        let h = spans("", &src, Some("ini"));
        assert_eq!(role_at(&h[0], 3), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[1], 0), SyntaxRole::String, "key");
        assert_eq!(role_at(&h[1], 11), SyntaxRole::Keyword, "boolean");
    }

    #[test]
    fn unknown_language_is_plain() {
        let src = lines(&["anything at all"]);
        let h = spans("", &src, Some("nosuchlang"));
        assert_eq!(h.len(), 1);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn no_language_is_plain() {
        let src = lines(&["plain text"]);
        let h = spans("", &src, None);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn spans_until_past_deadline_computes_nothing() {
        let src = lines(&["fn main() {", "}"]);
        let h = spans_until("", &src, Some("rust"), Some(Instant::now()));
        assert!(h.is_empty());
    }

    #[test]
    fn spans_until_without_deadline_matches_spans() {
        let src = lines(&["fn main() {", "    let s = \"hi\";", "}"]);
        assert_eq!(
            spans_until("", &src, Some("rust"), None),
            spans("", &src, Some("rust"))
        );
    }

    #[test]
    fn chunked_spans_concatenate_to_one_shot() {
        let src = CodeBody::from_text(
            &(0..10)
                .map(|i| format!("let x{i} = {i}; // n\n"))
                .collect::<String>(),
        );
        let mut starts = Vec::new();
        let mut joined = Vec::new();
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            4,
            None,
            || true,
            |c| {
                starts.push(c.start_line);
                joined.extend(c.spans);
                true
            },
        );
        assert!(complete);
        assert_eq!(starts, vec![0, 4, 8]);
        assert_eq!(joined, spans("", &src, Some("rust")));
    }

    #[test]
    fn chunked_spans_stop_between_chunks_when_told() {
        let src = CodeBody::from_text(
            &(0..10)
                .map(|i| format!("let x{i} = {i};\n"))
                .collect::<String>(),
        );
        let mut deliveries = 0;
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            4,
            None,
            || true,
            |_| {
                deliveries += 1;
                false
            },
        );
        assert!(!complete);
        assert_eq!(deliveries, 1);
    }

    #[test]
    fn worker_delivers_every_line_in_order() {
        let src = CodeBody::from_text(
            &(0..1200)
                .map(|i| format!("let v{i} = {i};\n"))
                .collect::<String>(),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.start(
            vec![PendingBlock {
                block: 3,
                source: Arc::from(""),
                language: Some("rust".into()),
                lines: src.clone(),
                resume: None,
            }],
            move || {
                let _ = tx.send(());
            },
        );
        let mut got: Vec<Arrival> = Vec::new();
        while got.iter().map(|a| a.spans.len()).sum::<usize>() < src.len() {
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("worker wake");
            got.extend(h.drain());
        }
        assert!(got.iter().all(|a| a.block == 3));
        assert_eq!(got.first().map(|a| a.start_line), Some(0));
        let joined: Vec<LineSpans> = got.into_iter().flat_map(|a| a.spans).collect();
        assert_eq!(joined, spans("", &src, Some("rust")));
    }

    #[test]
    fn restart_discards_stale_arrivals() {
        let src = CodeBody::from_text(
            &(0..600)
                .map(|i| format!("let v{i} = {i};\n"))
                .collect::<String>(),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.start(
            vec![PendingBlock {
                block: 0,
                source: Arc::from(""),
                language: None,
                lines: src,
                resume: None,
            }],
            move || {
                let _ = tx.send(());
            },
        );
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("first wake");
        h.start(Vec::new(), || {});
        assert!(h.drain().is_empty());
    }

    fn let_lines(n: usize) -> CodeBody {
        CodeBody::from_text(
            &(0..n)
                .map(|i| format!("let v{i} = {i}; // n\n"))
                .collect::<String>(),
        )
    }

    /// Every (line, seam) pair a fresh sweep delivers at the given
    /// chunk cadence.
    fn sweep_seams(src: &CodeBody, chunk: usize) -> Vec<(usize, Seam)> {
        let mut seams = Vec::new();
        spans_chunked(
            "",
            src,
            Some("rust"),
            chunk,
            None,
            || true,
            |c| {
                seams.push((c.start_line + c.spans.len(), c.seam));
                true
            },
        );
        seams
    }

    #[test]
    fn a_resumed_sweep_matches_the_full_parse_tail() {
        let src = let_lines(1200);
        let mut full = Vec::new();
        let mut seams = Vec::new();
        spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            None,
            || true,
            |c| {
                seams.push((c.start_line + c.spans.len(), c.seam));
                full.extend(c.spans);
                true
            },
        );
        assert_eq!(
            seams.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1024, 1200],
            "seams land at the chunk boundaries"
        );
        let (line, seam) = seams[0].clone();
        let resume = Resume {
            start_line: line,
            seam: Some(seam),
            expected: Vec::new(),
            cuts: Vec::new(),
        };
        let mut tail = Vec::new();
        spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                tail.extend(c.spans);
                true
            },
        );
        assert_eq!(tail, full[line..]);
    }

    #[test]
    fn the_early_stop_fires_at_the_first_matching_boundary() {
        let src = let_lines(2000);
        let seams = sweep_seams(&src, 512);
        let (line, seam) = seams[0].clone();
        let resume = Resume {
            start_line: line,
            seam: Some(seam),
            expected: seams,
            cuts: Vec::new(),
        };
        let mut chunks = Vec::new();
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                chunks.push(c);
                true
            },
        );
        assert!(complete, "a converged stop completes the block");
        assert_eq!(chunks.len(), 1, "one chunk re-colors and the sweep stops");
        assert!(chunks[0].converged);
        assert_eq!(chunks[0].start_line, 512);
        assert_eq!(chunks[0].spans.len(), 512);
    }

    #[test]
    fn a_state_changing_prefix_prevents_the_early_stop() {
        // The stored table describes the original text; the edited text
        // opens a comment at line 400 that never closes, so no computed
        // seam matches and the sweep runs to the block's end.
        let expected = sweep_seams(&let_lines(1200), 512);
        let mut text: Vec<String> = (0..1200).map(|i| format!("let v{i} = {i}; // n")).collect();
        text[400] = "/*".into();
        let edited = CodeBody::from_text(&text.join("\n"));
        let seams = sweep_seams(&edited, 512);
        let (line, seam) = seams[0].clone();
        let resume = Resume {
            start_line: line,
            seam: Some(seam),
            expected,
            cuts: Vec::new(),
        };
        let mut chunks = Vec::new();
        let complete = spans_chunked(
            "",
            &edited,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                chunks.push(c);
                true
            },
        );
        assert!(complete);
        assert!(chunks.iter().all(|c| !c.converged));
        assert_eq!(
            chunks.iter().map(|c| c.spans.len()).sum::<usize>(),
            1200 - 512,
            "the sweep colored everything from the resume line down"
        );
    }

    #[test]
    fn chunks_cut_to_the_stored_seam_lines() {
        // After an edit shifts the table off the chunk cadence, the
        // sweep cuts its chunks at the stored lines so the comparison
        // lands line-exact.
        let src = let_lines(1200);
        let fine = sweep_seams(&src, 100);
        let coarse = sweep_seams(&src, 512);
        let seam700 = fine.iter().find(|s| s.0 == 700).unwrap().clone();
        let (line, seam) = coarse[0].clone();
        let resume = Resume {
            start_line: line,
            seam: Some(seam),
            expected: vec![seam700],
            cuts: Vec::new(),
        };
        let mut chunks = Vec::new();
        spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                chunks.push(c);
                true
            },
        );
        assert_eq!(
            chunks[0].spans.len(),
            188,
            "the chunk stops at the stored line"
        );
        assert!(chunks[0].converged);
        assert_eq!(
            chunks[0].reach, 700,
            "the stop vouches up to the table's reach"
        );
        // The table stops at 700, so the sweep goes on from there.
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].start_line, 700);
        assert_eq!(chunks[1].spans.len(), 500, "to the block's end");
    }

    #[test]
    fn the_worker_resumes_and_reports_the_converged_stop() {
        let src = let_lines(1200);
        let seams = sweep_seams(&src, CHUNK_LINES);
        let (line, seam) = seams[0].clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.start(
            vec![PendingBlock {
                block: 0,
                source: Arc::from(""),
                language: Some("rust".into()),
                lines: src,
                resume: Some(Resume {
                    start_line: line,
                    seam: Some(seam),
                    expected: seams.clone(),
                    cuts: Vec::new(),
                }),
            }],
            move || {
                let _ = tx.send(());
            },
        );
        let mut got: Vec<Arrival> = Vec::new();
        while !got.iter().any(|a| a.converged) {
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("worker wake");
            got.extend(h.drain());
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start_line, 512);
        assert!(!got[0].speculative);
        assert_eq!(got[0].seam, Some(seams[1].1.clone()));
        for _ in 0..20_000 {
            if !h.is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!h.is_running(), "a converged stop completes the generation");
    }

    /// A table that stops short of the block's end, as a keystroke
    /// during the first sweep leaves it: the resumed sweep converges,
    /// names the table's reach, and goes on from the table's last seam,
    /// so the tail gets its colors.
    #[test]
    fn a_converged_stop_goes_on_past_the_table_s_reach() {
        let src = let_lines(3000);
        let seams: Vec<(usize, Seam)> =
            sweep_seams(&src, CHUNK_LINES).into_iter().take(3).collect();
        assert_eq!(seams.last().map(|s| s.0), Some(1536));
        let resume = Resume {
            start_line: 0,
            seam: None,
            expected: seams,
            cuts: Vec::new(),
        };
        let mut delivered: Vec<(usize, usize, bool, usize)> = Vec::new();
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            CHUNK_LINES,
            Some(&resume),
            || true,
            |c| {
                delivered.push((
                    c.start_line,
                    c.start_line + c.spans.len(),
                    c.converged,
                    c.reach,
                ));
                true
            },
        );
        assert!(complete);
        let converged: Vec<_> = delivered.iter().filter(|d| d.2).collect();
        assert_eq!(converged.len(), 1, "one converged stop: {delivered:?}");
        assert_eq!(
            converged[0].3, 1536,
            "the stop vouches up to the table's reach"
        );
        assert!(
            delivered.iter().any(|d| d.0 == 1536),
            "the sweep goes on from the table's last seam: {delivered:?}"
        );
        assert_eq!(
            delivered.last().map(|d| d.1),
            Some(3000),
            "and reaches the block's end: {delivered:?}"
        );
    }

    /// A line kept across an edit, the burst's end among them, moves
    /// as a seam does: unmoved before the edit, the region's new end
    /// inside it, moved by the delta past it.
    #[test]
    fn a_line_moves_with_the_lines_of_an_edit() {
        assert_eq!(shift_line(100, &(600..603), &(600..601)), 100);
        assert_eq!(shift_line(600, &(600..603), &(600..601)), 600);
        assert_eq!(
            shift_line(602, &(600..603), &(600..601)),
            601,
            "a line inside the replaced lines is their new end"
        );
        assert_eq!(
            shift_line(1000, &(600..603), &(600..601)),
            998,
            "a line past the edit moves by the delta"
        );
        assert_eq!(shift_line(1000, &(512..513), &(512..515)), 1002);
    }

    /// A table out of order, which no sweep of the application files
    /// but the function takes: the sweep ends and reaches the block's
    /// end, instead of starting again below the chunk it converged on.
    #[test]
    fn a_table_out_of_order_ends_the_sweep() {
        let src = let_lines(3000);
        let mut seams: Vec<(usize, Seam)> =
            sweep_seams(&src, CHUNK_LINES).into_iter().take(3).collect();
        seams.reverse();
        assert_eq!(seams.first().map(|s| s.0), Some(1536));
        let resume = Resume {
            start_line: 0,
            seam: None,
            expected: seams,
            cuts: Vec::new(),
        };
        let mut delivered = 0;
        let mut last_end = 0;
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            CHUNK_LINES,
            Some(&resume),
            || true,
            |c| {
                delivered += 1;
                last_end = c.start_line + c.spans.len();
                delivered <= 20
            },
        );
        assert!(
            complete,
            "the sweep ends by itself: {delivered} deliveries, the last to {last_end}"
        );
        assert_eq!(last_end, 3000, "and reaches the block's end");
    }

    /// The walk of a key: from a stored seam toward the end of the
    /// touched lines, with the budget to spare, the spans equal the
    /// full parse and the walk reaches the end.
    #[test]
    fn a_walk_from_a_seam_colors_as_the_full_parse_does() {
        let src = let_lines(700);
        let full = spans("", &src, Some("rust"));
        let seams = sweep_seams(&src, 512);
        let (line, seam) = seams[0].clone();
        let walked = walk(
            "",
            &src,
            Some("rust"),
            line,
            Some(seam),
            600,
            603,
            std::time::Duration::from_secs(60),
        );
        assert_eq!(walked.start_line, 512);
        assert_eq!(walked.reached, 603, "the end of the touched lines");
        assert_eq!(walked.spans, full[512..603]);
    }

    /// The states a walk records, before the mark and at its end, are
    /// the seams a chunked sweep files at those lines: a sweep resumed
    /// against them converges.
    #[test]
    fn a_walk_records_the_states_a_sweep_would_file() {
        let src = let_lines(700);
        let seams = sweep_seams(&src, 512);
        let (line, seam) = seams[0].clone();
        let walked = walk(
            "",
            &src,
            Some("rust"),
            line,
            Some(seam.clone()),
            600,
            603,
            std::time::Duration::from_secs(60),
        );
        let at_mark = walked.at_mark.expect("the state before the mark");
        for (at, state) in [(600, at_mark), (603, walked.seam)] {
            let resume = Resume {
                start_line: line,
                seam: Some(seam.clone()),
                expected: vec![(at, state)],
                cuts: Vec::new(),
            };
            let mut converged = false;
            spans_chunked(
                "",
                &src,
                Some("rust"),
                512,
                Some(&resume),
                || true,
                |c| {
                    converged |= c.converged;
                    !c.converged
                },
            );
            assert!(
                converged,
                "the state before line {at} is what a sweep finds there"
            );
        }
    }

    /// A walk with no budget parses one line and stops; its seam is the
    /// state before the next line, where the next walk continues.
    #[test]
    fn a_walk_out_of_budget_stops_after_one_line() {
        let src = let_lines(100);
        let walked = walk(
            "",
            &src,
            Some("rust"),
            0,
            None,
            50,
            51,
            std::time::Duration::ZERO,
        );
        assert_eq!(walked.reached, 1);
        assert_eq!(walked.spans.len(), 1);
        assert!(walked.at_mark.is_none(), "the mark was not reached");
        let next = walk(
            "",
            &src,
            Some("rust"),
            1,
            Some(walked.seam),
            50,
            51,
            std::time::Duration::from_secs(60),
        );
        assert_eq!(next.reached, 51);
        let full = spans("", &src, Some("rust"));
        assert_eq!(next.spans, full[1..51]);
        assert!(next.at_mark.is_some(), "the mark passed on the way");
    }

    /// A sweep cuts a chunk at the lines it is asked to, so that a seam
    /// is filed there, and converges against the table as before.
    #[test]
    fn a_sweep_cuts_at_the_asked_lines() {
        let src = let_lines(1200);
        let seams = sweep_seams(&src, 512);
        let resume = Resume {
            start_line: 0,
            seam: None,
            expected: seams,
            cuts: vec![100, 103],
        };
        let mut ends = Vec::new();
        spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                ends.push(c.start_line + c.spans.len());
                true
            },
        );
        assert_eq!(
            ends,
            vec![100, 103, 512],
            "cut at the asked lines, then converged at the table's first seam"
        );
    }

    /// A killed sweep stops at the next line, and nothing of the chunk
    /// in progress is delivered.
    #[test]
    fn a_killed_sweep_stops_at_the_next_line() {
        let src = let_lines(1200);
        let mut asked = 0usize;
        let mut delivered = 0;
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            None,
            || {
                asked += 1;
                asked <= 10
            },
            |_| {
                delivered += 1;
                true
            },
        );
        assert!(!complete);
        assert_eq!(delivered, 0, "the chunk in progress is not delivered");
        assert_eq!(asked, 11, "the kill is read before each line");
    }

    /// The seams between the frontier and the burst's end describe lines
    /// the burst changed: they are dropped before a resume, and the ones
    /// at or before the frontier and past the burst stand.
    #[test]
    fn the_stale_seams_of_a_burst_are_dropped() {
        let mut table = sweep_seams(&let_lines(3000), CHUNK_LINES);
        drop_stale_seams(&mut table, 1024, 2047);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1024, 2048, 2560, 3000]
        );
        drop_stale_seams(&mut table, 600, 600);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1024, 2048, 2560, 3000],
            "a burst past no seam drops nothing"
        );
    }

    #[test]
    fn the_table_stays_ordered_and_free_of_duplicates() {
        let seams = sweep_seams(&let_lines(2000), CHUNK_LINES);
        let mut table = Vec::new();
        // Filed out of order, as a resumed sweep's arrivals reach a
        // table the first sweep already seeded.
        for (line, seam) in seams.iter().rev() {
            record_seam(&mut table, *line, seam);
        }
        assert_eq!(
            table.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![512, 1024, 1536, 2000],
            "every seam is kept, ordered by line"
        );
        let (line, seam) = table[1].clone();
        record_seam(&mut table, line, &seam);
        assert_eq!(table.len(), 4, "a re-swept line updates in place");
    }

    #[test]
    fn speculation_colors_the_band_from_the_cold_guess() {
        let src = let_lines(1200);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.speculate(
            SpecJob {
                block: 0,
                source: Arc::from(""),
                language: Some("rust".into()),
                lines: src.clone(),
                range: 800..900,
            },
            move || {
                let _ = tx.send(());
            },
        );
        assert!(
            !h.is_running(),
            "speculation is not the exact wash; an export never waits on it"
        );
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("speculation wake");
        let got = h.drain();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start_line, 800);
        assert_eq!(got[0].spans.len(), 100);
        assert!(got[0].speculative);
        assert!(
            got[0].seam.is_none(),
            "a guessed state never becomes a seam"
        );
        assert!(!got[0].converged);
        // These lines sit at top level, where the cold guess is exact.
        let truth = spans("", &src, Some("rust"));
        assert_eq!(got[0].spans, truth[800..900]);
    }

    #[test]
    fn a_document_swap_drops_speculation_in_flight() {
        let src = let_lines(4000);
        let mut h = Highlighter::new();
        h.speculate(
            SpecJob {
                block: 0,
                source: Arc::from(""),
                language: Some("rust".into()),
                lines: src,
                range: 0..4000,
            },
            || {},
        );
        h.start(Vec::new(), || {});
        for _ in 0..2_000 {
            assert!(h.drain().is_empty(), "a superseded speculation never lands");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn the_seam_table_shifts_across_an_edit() {
        let seams = sweep_seams(&let_lines(1600), 512);
        assert_eq!(
            seams.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1024, 1536, 1600]
        );
        let mut table = seams.clone();
        shift_seams(&mut table, 600..603, 600..601);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1022, 1534, 1598],
            "entries before the edit stand, entries past it shift"
        );
        let mut table = seams.clone();
        shift_seams(&mut table, 1000..1030, 1000..1030);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1536, 1600],
            "an entry inside the touched range drops"
        );
        let mut table = seams.clone();
        shift_seams(&mut table, 512..513, 512..515);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            vec![512, 1026, 1538, 1602],
            "an entry at the touched region's first line stands"
        );
    }

    #[test]
    fn a_resume_without_a_seam_starts_fresh_and_still_converges() {
        let src = let_lines(1200);
        let seams = sweep_seams(&src, 512);
        let resume = Resume {
            start_line: 0,
            seam: None,
            expected: seams,
            cuts: Vec::new(),
        };
        let mut chunks = Vec::new();
        let complete = spans_chunked(
            "",
            &src,
            Some("rust"),
            512,
            Some(&resume),
            || true,
            |c| {
                chunks.push(c);
                true
            },
        );
        assert!(complete);
        assert_eq!(chunks.len(), 1, "the first boundary already converges");
        assert!(chunks[0].converged);
        assert_eq!(chunks[0].start_line, 0);
    }

    #[test]
    fn guess_hits_at_top_level_and_misses_inside_a_construct() {
        let src = lines(&[
            "fn a() {}",
            "/*",
            "still a comment",
            "*/",
            "fn b() {}",
            "fn c() {}",
        ]);
        let probes = segment_probe("", &src, Some("rust"), 2);
        let hits: Vec<bool> = probes.iter().map(|p| p.state_hit).collect();
        assert_eq!(
            hits,
            vec![false, true],
            "line 2 is inside the comment, line 4 is back at top level"
        );
        assert!(
            probes[0].drifted_lines > 0,
            "a cold start inside the comment colors its lines differently"
        );
        assert_eq!(
            probes[1].drifted_lines, 0,
            "a cold start at top level colors identically"
        );
        assert_eq!(probes[0].lines, 2);
    }

    #[test]
    fn empty_lines_produce_empty_ranges() {
        let src = lines(&["", "x"]);
        let h = spans("", &src, Some("rust"));
        assert_eq!(h.len(), 2);
        assert!(h[0].is_empty());
    }

    #[test]
    fn a_task_box_followed_by_a_parenthesized_word_is_not_a_link() {
        for (line, texts) in [
            (
                "- [ ] (Ivan) FLEX update reporting, finish within 2 weeks (Soumya)",
                ["[ ]", "(Ivan)", "(Soumya)"],
            ),
            ("- [x] (Aldwin) day 1 or day 2?", ["[x]", "(Aldwin)", "day"]),
            ("see [text] [ref] here", ["[text]", "[ref]", "here"]),
            (
                "![alt text] (img.png)",
                ["![alt text]", "(img.png)", "img.png"],
            ),
        ] {
            let src = lines(&[line]);
            let h = spans("", &src, Some("md"));
            for text in texts {
                let at = line.find(text).unwrap();
                for pos in at..at + text.len() {
                    assert_eq!(
                        role_at(&h[0], pos),
                        SyntaxRole::Plain,
                        "{line:?} byte {pos} ({:?}) of {text:?} is plain text, not a link",
                        &line[pos..pos + 1]
                    );
                }
            }
        }
    }

    /// A file with no line break is one long first line: the rules read
    /// its head, cut on a character, and a shebang there still answers.
    #[test]
    fn a_first_line_of_any_length_is_read_by_its_head() {
        let long = format!("#!/bin/sh {}", "é".repeat(4 * FIRST_LINE_READ));
        assert_eq!(
            sniff_language("run", &long),
            Some("Bourne Again Shell (bash)")
        );
        assert_eq!(sniff_language("data", &"é".repeat(FIRST_LINE_READ)), None);
    }

    /// A file with no extension says what it is in four ways, tried in
    /// order: its first line, a modeline, its name, or its shape; prose
    /// says nothing and stays plain.
    #[test]
    fn a_file_without_an_extension_is_sniffed_from_what_it_says() {
        let sniff = |name: &str, text: &str| sniff_language(name, text);
        assert_eq!(
            sniff("script", "#!/usr/bin/env python3\nprint(1)\n"),
            Some("Python")
        );
        assert_eq!(
            sniff("run", "#!/bin/sh\necho hi\n"),
            Some("Bourne Again Shell (bash)")
        );
        assert_eq!(
            sniff("feed", "<?xml version=\"1.0\"?>\n<rss/>\n"),
            Some("XML")
        );
        assert_eq!(
            sniff("modeline", "# a comment\n# vim: set ft=ruby:\nputs 1\n"),
            Some("Ruby")
        );
        assert_eq!(
            sniff("tail", "puts 1\nputs 2\n# vim:ft=ruby\n"),
            Some("Ruby"),
            "a modeline in the last lines counts too"
        );
        assert_eq!(
            sniff("emacs", ";; -*- mode: lisp -*-\n(defun f () 1)\n"),
            Some("Lisp")
        );
        assert_eq!(
            sniff("emacs-bare", "# -*- python -*-\nprint(1)\n"),
            Some("Python"),
            "the bare Emacs form names the mode alone"
        );
        assert_eq!(
            sniff(".bashrc", "# no shebang\nexport A=1\n"),
            Some("Bourne Again Shell (bash)"),
            "the grammars know the name"
        );
        assert_eq!(
            sniff("Gemfile", "source 'https://rubygems.org'\n"),
            Some("Ruby")
        );
        assert_eq!(
            sniff(
                "patch",
                "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"
            ),
            Some("Diff")
        );
        assert_eq!(
            sniff("changes", "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"),
            Some("Diff"),
            "a unified diff without the git header"
        );
        assert_eq!(
            sniff("settings", "{\n  \"theme\": \"nord\"\n}\n"),
            Some("JSON")
        );
        assert_eq!(sniff("config", "[window]\nwidth = 1000\n"), Some("INI"));
        assert_eq!(sniff("gitconfig", "[user]\n\tname = x\n"), Some("INI"));
        assert_eq!(
            sniff("page", "<html>\n<body>hi</body>\n</html>\n"),
            Some("HTML"),
            "the grammars' first-line patterns know an html tag"
        );
        assert_eq!(
            sniff("feed", "<rss>\n<item/>\n</rss>\n"),
            Some("XML"),
            "a tag the patterns do not know is XML by its shape"
        );
        assert_eq!(
            sniff("README", "Oryx\n\nA fast viewer.\n"),
            None,
            "prose stays plain"
        );
        assert_eq!(
            sniff("notes", "[a link](x)\n\nmore\n"),
            None,
            "a bracket alone is not INI"
        );
        assert_eq!(
            sniff("list", "[\"a\", \"b\"]\n"),
            None,
            "an array of strings is not JSON enough"
        );
        assert_eq!(sniff("empty", ""), None);
    }
}

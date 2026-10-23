//! File loading and type detection by extension.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::doc::model::{Block, BlockKind, CodeBody, Document, Span};
use crate::style::highlight::{self, Arrival, PendingBlock, Seam};
use std::collections::HashMap;

/// Sync highlighting budget at open; whatever remains goes to the
/// background worker.
pub const OPEN_BUDGET: Duration = Duration::from_millis(40);

/// How many leading bytes the binary test looks at.
pub const SNIFF: usize = 8192;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FileKind {
    Markdown,
    /// Carries the syntax token used for highlighting (`"rust"`, `"python"`).
    Code(&'static str),
    /// Prose, line-oriented like code, drawn in the body face.
    Text,
    /// An EPUB book; a zip, so it never meets the binary sniff.
    Epub,
    /// An FB2 book, plain XML or zip-wrapped; routed before the sniff.
    Fb2,
    /// A Kindle book (MOBI, AZW3, AZW); a Palm database, never text.
    Kindle,
    /// A comic book archive (CBZ or CBR); a zip or RAR of page images,
    /// so it never meets the binary sniff. The reader dispatches on the
    /// signature, since the wild mislabels the two freely.
    Comic,
    /// A format Oryx knows it cannot display (PDF). Refused by name:
    /// some PDFs open with an all-ASCII head the content sniff passes.
    Undisplayable,
    /// Not identified by extension. Its content decides: text opens as code
    /// with no language, binary is refused.
    Unknown,
}

/// The kind of a file with its text at hand: `detect` by name, and for
/// a name that says nothing, what the text says about itself (a
/// shebang, a modeline, a diff header, a JSON or INI shape) as a code
/// file of that grammar. Plain when nothing answers. The open path and
/// the editor's round trip use it; the sidebar keeps the cheap `detect`.
pub fn detect_with_text(path: &Path, text: &str) -> FileKind {
    match detect(path) {
        FileKind::Unknown => {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            match crate::style::highlight::sniff_language(name, text) {
                Some(grammar) => FileKind::Code(grammar),
                None => FileKind::Unknown,
            }
        }
        kind => kind,
    }
}

pub fn detect(path: &Path) -> FileKind {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Ok(i) = WELL_KNOWN_NAMES.binary_search_by_key(&name, |(k, _)| k) {
            return FileKind::Code(WELL_KNOWN_NAMES[i].1);
        }
        // The double extension libraries ship FB2 under; `.zip` alone
        // stays unknown.
        if name.to_ascii_lowercase().ends_with(".fb2.zip") {
            return FileKind::Fb2;
        }
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return FileKind::Unknown;
    };
    let ext = ext.to_ascii_lowercase();
    if ext == "md" || ext == "markdown" {
        return FileKind::Markdown;
    }
    if ext == "txt" {
        return FileKind::Text;
    }
    if ext == "epub" {
        return FileKind::Epub;
    }
    if ext == "fb2" || ext == "fbz" {
        return FileKind::Fb2;
    }
    if ext == "mobi" || ext == "azw3" || ext == "azw" {
        return FileKind::Kindle;
    }
    if ext == "cbz" || ext == "cbr" {
        return FileKind::Comic;
    }
    if ext == "pdf" {
        return FileKind::Undisplayable;
    }
    match CODE_EXTENSIONS.binary_search_by_key(&ext.as_str(), |(k, _)| k) {
        Ok(i) => FileKind::Code(CODE_EXTENSIONS[i].1),
        Err(_) => FileKind::Unknown,
    }
}

/// A NUL byte near the start is the standard test, the one `git` and
/// `grep -I` use: no text encoding Oryx renders produces one, and every
/// container and executable format has one in its header. A file whose
/// head has none but is mostly bytes no text reader can show, invalid
/// UTF-8 and control characters, is binary too: a cartridge ROM whose
/// first zero sits past the window read that way, and its random
/// characters had the shaper load every font on the system. Text keeps
/// such bytes rare, a heavily accented Latin-1 file at 17 percent, so
/// the line is drawn at 30.
fn is_binary(bytes: &[u8]) -> bool {
    refusal(bytes).is_some()
}

/// Why a file's head is refused, in the words that end the message. A
/// zero byte is no text in any encoding. A head of unreadable bytes
/// without one may be text in an encoding older than UTF-8 (Cyrillic
/// in CP1251 reads 83 percent unreadable, Japanese in Shift-JIS 55),
/// which Oryx does not read: the message says what is true of both.
fn refusal(bytes: &[u8]) -> Option<&'static str> {
    let head = &bytes[..bytes.len().min(SNIFF)];
    if head.contains(&0) {
        return Some("is not a text file");
    }
    let controls = |text: &[u8]| {
        text.iter()
            .filter(|&&b| {
                (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0c | 0x1b)) || b == 0x7f
            })
            .count()
    };
    let mut unreadable = 0;
    let mut rest = head;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(_) => {
                unreadable += controls(rest);
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                unreadable += controls(&rest[..valid]);
                // A sequence the window cut short is the window's fault,
                // not the file's.
                let Some(bad) = err.error_len() else {
                    break;
                };
                unreadable += bad;
                rest = &rest[valid + bad..];
            }
        }
    }
    (unreadable * 10 > head.len() * 3).then_some("is not UTF-8 text")
}

/// Whether a file on disk holds text, read from its first bytes. A file
/// that cannot be read is not displayable either, so it answers false.
pub fn is_text_file(path: &Path) -> bool {
    use std::io::Read;
    if detect(path) == FileKind::Undisplayable {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; SNIFF];
    match file.read(&mut head) {
        Ok(n) => !is_binary(&head[..n]),
        Err(_) => false,
    }
}

/// A book's continuation past the open prefix, whichever format built
/// it. The app drives it through one surface; each variant walks its
/// own container.
pub enum BookJob {
    Epub(super::epub::BookJob),
    Fb2(super::fb2::Job),
    Kindle(super::kindle::Job),
    /// Carries the page sources only; a comic streams nothing.
    Comic(super::comic::Job),
}

impl BookJob {
    pub fn has_chapters(&self) -> bool {
        match self {
            BookJob::Epub(job) => job.has_chapters(),
            BookJob::Fb2(job) => job.has_chapters(),
            BookJob::Kindle(job) => job.has_chapters(),
            BookJob::Comic(job) => job.has_chapters(),
        }
    }

    pub fn take_sources(&mut self) -> Vec<super::images::SourceEntry> {
        match self {
            BookJob::Epub(job) => job.take_sources(),
            BookJob::Fb2(job) => job.take_sources(),
            BookJob::Kindle(job) => job.take_sources(),
            BookJob::Comic(job) => job.take_sources(),
        }
    }

    /// Continues the book on the parse worker; the delivery is the full
    /// model over the grown source.
    pub fn run(
        self,
        bail: &dyn Fn() -> bool,
        sources: super::images::SourceSink,
    ) -> Option<super::stream::Delivered> {
        match self {
            BookJob::Epub(job) => super::epub::run(job, bail, sources),
            BookJob::Fb2(job) => super::fb2::run(job, bail, sources),
            BookJob::Kindle(job) => super::kindle::run(job, bail, sources),
            // Never reached: a comic has no chapters, so no worker starts.
            BookJob::Comic(_) => None,
        }
    }
}

/// An opened document and the code blocks whose highlighting did not
/// finish inside the deadline.
pub struct Opened {
    pub document: Document,
    pub pending: Vec<PendingBlock>,
    /// True when the document holds only a parsed prefix and the parse
    /// worker owes the rest; the blocks cover the source up to the cut.
    pub streamed: bool,
    /// A book's continuation: the walk past the prefix and the images
    /// not yet decoded. The app starts it once the media cache exists.
    pub book: Option<BookJob>,
    /// A book's table of contents as authored; empty for files.
    pub toc: Vec<super::epub::TocEntry>,
    /// True when the bytes decoded only through lossy UTF-8
    /// replacement. Editing refuses such a file: byte fidelity cannot
    /// be promised back to disk over a lossy read.
    pub lossy: bool,
    /// Offsets in the normalized text of each newline the file bytes
    /// had returns before, one entry per return: a Windows line break
    /// lists its newline once, a damaged CR CR LF twice. The splice
    /// ledger restores them on save, so the normalization the viewer
    /// needs never reaches the disk.
    pub crlf: Vec<u32>,
    /// The file breaks its lines with CR alone, the classic Mac OS way,
    /// and has no LF at all: the text reads with LF in their place, and
    /// the splice ledger writes CR back on save, for new lines too. A
    /// file with any LF keeps a lone CR as text.
    pub cr: bool,
    /// The file opened with a UTF-8 byte order mark, stripped from the
    /// text; the splice ledger writes it back first on save.
    pub bom: bool,
}

pub fn open(path: &Path, deadline: Option<Instant>) -> anyhow::Result<Opened> {
    let bytes =
        std::fs::read(path).map_err(|e| anyhow::anyhow!("cannot open {}: {e}", path.display()))?;
    if detect(path) == FileKind::Undisplayable {
        anyhow::bail!(
            "{} is a PDF file; Oryx does not display PDF files",
            path.display()
        );
    }
    // A book may be a zip full of NUL bytes; it routes before the sniff.
    let book_kind = detect(path);
    if matches!(
        book_kind,
        FileKind::Epub | FileKind::Fb2 | FileKind::Kindle | FileKind::Comic
    ) {
        let (mut document, toc, book) = match book_kind {
            FileKind::Epub => {
                let (document, toc, job) = super::epub::open_prefix(bytes)?;
                (document, toc, job.map(BookJob::Epub))
            }
            FileKind::Kindle => {
                let (document, toc, job) = super::kindle::open_prefix(bytes)?;
                (document, toc, job.map(BookJob::Kindle))
            }
            FileKind::Comic => {
                // A comic has no metadata; the file stem names the book.
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Comic");
                let (document, toc, job) = super::comic::open_prefix(bytes, stem)?;
                (document, toc, job.map(BookJob::Comic))
            }
            _ => {
                let (document, toc, job) = super::fb2::open_prefix(bytes)?;
                (document, toc, job.map(BookJob::Fb2))
            }
        };
        // Position memory falls back to the canonical path when the
        // metadata carries no identifier. A metadata id is shared by
        // every container of the same book while positions are offsets
        // into one container's text, so the key carries the extension;
        // EPUB keys predate the multi-format shelf and stay bare so no
        // saved place is lost.
        if document.book_id.is_none() {
            document.book_id = path.canonicalize().ok().map(|p| p.display().to_string());
        } else if book_kind != FileKind::Epub {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if let Some(id) = document.book_id.take() {
                    document.book_id = Some(format!("{id}|{}", ext.to_ascii_lowercase()));
                }
            }
        }
        let pending = apply_budget(&mut document, deadline);
        let streamed = book.as_ref().is_some_and(|job| job.has_chapters());
        return Ok(Opened {
            document,
            pending,
            streamed,
            book,
            toc,
            lossy: false,
            crlf: Vec::new(),
            cr: false,
            bom: false,
        });
    }
    if let Some(why) = refusal(&bytes) {
        anyhow::bail!("{} {why}", path.display());
    }
    let text = String::from_utf8_lossy(&bytes);
    let lossy = matches!(text, std::borrow::Cow::Owned(_));
    // A byte order mark is not text: left in, markdown reads it as the
    // first character of the first line and a heading there is lost.
    // It leaves the text and goes on record for the save to write back.
    let bom = text.starts_with('\u{FEFF}');
    let text = if bom {
        match text {
            std::borrow::Cow::Borrowed(t) => std::borrow::Cow::Borrowed(&t[3..]),
            std::borrow::Cow::Owned(t) => std::borrow::Cow::Owned(t[3..].to_string()),
        }
    } else {
        text
    };
    // Windows files carry CRLF, and a damaged file a run of returns
    // before each line break (CR CR LF, an old Mac and a Windows
    // conversion on top of each other). Every return before a line
    // break leaves the text: everything downstream (offsets, the
    // editor's line math, rendering, copy as markdown) assumes the
    // source is clean of them. Each stripped return leaves the line
    // break's normalized offset behind, one entry per return, so the
    // splice ledger can put every untouched ending back verbatim on
    // save. A return with no line break after it is text.
    let (text, crlf, cr) = if text.contains("\r\n") {
        let mut out = String::with_capacity(text.len());
        let mut crlf = Vec::new();
        for piece in text.split_inclusive('\n') {
            let Some(line) = piece.strip_suffix('\n') else {
                out.push_str(piece);
                break;
            };
            let head = line.trim_end_matches('\r');
            out.push_str(head);
            for _ in head.len()..line.len() {
                crlf.push(out.len() as u32);
            }
            out.push('\n');
        }
        (std::borrow::Cow::Owned(out), crlf, false)
    } else if text.contains('\r') && !text.contains('\n') {
        // A classic Mac OS file: CR alone breaks its lines.
        (
            std::borrow::Cow::Owned(text.replace('\r', "\n")),
            Vec::new(),
            true,
        )
    } else {
        (text, Vec::new(), false)
    };
    let mut streamed = false;
    let mut document = match detect(path) {
        // A markdown file past the prefix target parses only up to the
        // cut; the worker owes the rest and the swap lands it. The full
        // source rides along so every range is in final coordinates.
        FileKind::Markdown => match super::stream::cut(&text) {
            Some(cut) => {
                streamed = true;
                let mut prefix = super::markdown::parse(&text[..cut]);
                prefix.source = Arc::from(&*text);
                prefix
            }
            None => super::markdown::parse(&*text),
        },
        FileKind::Code(token) => code_document(Some(token), &text),
        FileKind::Text => text_document(&text),
        FileKind::Epub | FileKind::Fb2 | FileKind::Kindle | FileKind::Comic => {
            unreachable!("books returned before the sniff")
        }
        FileKind::Undisplayable => unreachable!("refused before the sniff"),
        FileKind::Unknown => match detect_with_text(path, &text) {
            FileKind::Code(grammar) => code_document(Some(grammar), &text),
            _ => code_document(None, &text),
        },
    };
    let pending = apply_budget(&mut document, deadline);
    Ok(Opened {
        document,
        pending,
        streamed,
        book: None,
        toc: Vec::new(),
        lossy,
        crlf,
        cr,
        bom,
    })
}

/// Every code block whose highlight prefix is incomplete, for restarting
/// the highlight worker after the parse swap. Unlike `apply_budget` it
/// leaves computed highlights alone.
pub fn pending(doc: &Document) -> Vec<PendingBlock> {
    let mut pending = Vec::new();
    for (index, block) in doc.blocks.iter().enumerate() {
        let BlockKind::CodeBlock {
            language,
            lines,
            exact,
            ..
        } = &block.kind
        else {
            continue;
        };
        if *exact < lines.len() {
            pending.push(PendingBlock {
                block: index,
                language: language.clone(),
                source: Arc::clone(&doc.source),
                lines: lines.clone(),
                resume: None,
            });
        }
    }
    pending
}

/// Every code block whose highlight prefix is incomplete, each with a
/// resume from its stored seam table when it has one: the sweep starts
/// at the seam at or before the exact frontier and converges against
/// the seams past it, and past `after`, the burst's end (zero for none),
/// when a later edit of the burst stands beyond the first of them.
/// `cuts`, the lines of the last key, end a chunk so that a seam is
/// filed there, on the one block of a code file or of the source view;
/// a page's blocks take none.
pub fn pending_from(
    doc: &Document,
    seams: &HashMap<usize, Vec<(usize, Seam)>>,
    after: usize,
    cuts: &[usize],
) -> Vec<PendingBlock> {
    let one_block = doc.blocks.len() == 1;
    pending(doc)
        .into_iter()
        .map(|mut block| {
            let exact = match &doc.blocks[block.block].kind {
                BlockKind::CodeBlock { exact, .. } => *exact,
                _ => 0,
            };
            block.resume = seams.get(&block.block).map(|table| {
                let at = table.partition_point(|(line, _)| *line <= exact);
                let (start_line, seam) = match at.checked_sub(1).and_then(|i| table.get(i)) {
                    Some((line, seam)) => (*line, Some(seam.clone())),
                    None => (0, None),
                };
                // A seam between the frontier and the burst's end stands
                // on lines a later edit of the burst changed: the sweep
                // colors them again and converges past them.
                let past = exact.max(after.saturating_sub(1));
                let from = table.partition_point(|(line, _)| *line <= past);
                highlight::Resume {
                    start_line,
                    seam,
                    expected: table[from..].to_vec(),
                    cuts: if one_block { cuts.to_vec() } else { Vec::new() },
                }
            });
            block
        })
        .collect()
}

/// Highlights code blocks in document order until the deadline, leaving
/// each block's computed prefix in place; returns the unfinished blocks.
fn apply_budget(doc: &mut Document, deadline: Option<Instant>) -> Vec<PendingBlock> {
    let mut pending = Vec::new();
    let source = Arc::clone(&doc.source);
    for (index, block) in doc.blocks.iter_mut().enumerate() {
        let BlockKind::CodeBlock {
            language,
            lines,
            highlights,
            exact,
        } = &mut block.kind
        else {
            continue;
        };
        *highlights = highlight::spans_until(&source, lines, language.as_deref(), deadline);
        *exact = highlights.len();
        if *exact < lines.len() {
            pending.push(PendingBlock {
                block: index,
                language: language.clone(),
                source: Arc::clone(&source),
                lines: lines.clone(),
                resume: None,
            });
        }
    }
    pending
}

/// Copies one arrived chunk into its block's highlight prefix, growing
/// the prefix when the chunk extends it. Out-of-range chunks and
/// non-code blocks are ignored; arrivals are trusted only as far as the
/// current document reaches.
pub fn fold(doc: &mut Document, arrival: &Arrival) {
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
    if highlights.len() < end {
        highlights.resize(end, Vec::new());
    }
    // A guess that lands after the exact sweep colored a line, with
    // colors from a cold start, leaves that line alone.
    let first = if arrival.speculative { *exact } else { 0 };
    for (offset, spans) in arrival.spans.iter().enumerate() {
        let line = arrival.start_line + offset;
        if first <= line && line < end {
            highlights[line] = spans.clone();
        }
    }
    // The exact frontier only ever advances contiguously, and a
    // converged stop vouches for the lines up to the stored table's
    // reach, which the earlier sweep colored: the block's end when that
    // sweep completed, less when a keystroke cut it short.
    if !arrival.speculative {
        if arrival.start_line <= *exact {
            *exact = (*exact).max(end).min(lines.len());
        }
        if arrival.converged {
            *exact = (*exact).max(arrival.reach).min(lines.len());
        }
    }
}

/// `text` with every return before a line break removed, the way the
/// load cleans a file: what pasted text goes through, so a line break
/// is always one byte inside the editor.
pub fn without_returns(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive('\n') {
        match piece.strip_suffix('\n') {
            Some(line) => {
                out.push_str(line.trim_end_matches('\r'));
                out.push('\n');
            }
            None => out.push_str(piece),
        }
    }
    out
}

/// A short notice (an open error) rendered as a plain document.
pub fn message(text: &str) -> Document {
    plain_document(text)
}

/// Every extension Oryx renders intentionally, for dialog filters.
pub fn recognized_extensions() -> Vec<&'static str> {
    [
        "md", "markdown", "txt", "epub", "fb2", "fbz", "mobi", "azw3", "azw", "cbz", "cbr",
    ]
    .into_iter()
    .chain(CODE_EXTENSIONS.iter().map(|(ext, _)| *ext))
    .collect()
}

fn source_lines(text: &str) -> Vec<Range<u32>> {
    let base = text.as_ptr() as usize;
    text.lines()
        .map(|line| {
            let start = (line.as_ptr() as usize - base) as u32;
            start..start + line.len() as u32
        })
        .collect()
}

/// The whole file as a single code block; the budget pass highlights it.
/// No token means no grammar, so the block renders in the code font
/// unstyled. The lines are ranges into the source, so the file's text
/// is held once. Every line is a row, trailing empty lines included, so
/// an Enter at the end of the file opens a row the page can scroll to;
/// the final newline ends the last row and opens none.
pub(crate) fn code_document(token: Option<&str>, text: &str) -> Document {
    let mut block = Block::plain(BlockKind::CodeBlock {
        language: token.map(str::to_string),
        lines: CodeBody::verbatim(source_lines(text)),
        highlights: Vec::new(),
        exact: 0,
    });
    block.range = 0..text.len();
    Document {
        blocks: vec![block],
        source: Arc::from(text),
        code_file: true,
        ..Document::default()
    }
}

/// The whole file as one line-oriented block, like a code file, drawn
/// as prose: the body face, the page background, no panel, no
/// highlighting. Every line is a row, trailing blank lines included,
/// so editing sees exactly the lines the file has.
pub(crate) fn text_document(text: &str) -> Document {
    let mut block = Block::plain(BlockKind::CodeBlock {
        language: None,
        lines: CodeBody::verbatim(source_lines(text)),
        highlights: Vec::new(),
        exact: 0,
    });
    block.range = 0..text.len();
    Document {
        blocks: vec![block],
        source: Arc::from(text),
        plain_file: true,
        ..Document::default()
    }
}

/// Paragraphs split on blank lines; line breaks inside a paragraph are
/// preserved as newline spans so the lines sit flush in layout. A blank
/// line is a row of its own, carried as a newline span on the open
/// paragraph, so the page shows every line the file has; plain files
/// draw with no added gap between blocks.
pub(crate) fn plain_document(text: &str) -> Document {
    let mut blocks = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut trailing = false;
    let mut offset = 0;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            // The final split fragment past the file's terminator is
            // not a line; a real blank row always has its newline byte.
            let term = offset + raw.len();
            if term < text.len() {
                // The layout consumes one newline span as the segment
                // break after a text line; the blank's own row needs
                // its separator first.
                if !trailing && !spans.is_empty() {
                    spans.push(Span::plain("\n"));
                }
                let mut span = Span::plain("\n");
                span.range = term as u32..(term + 1) as u32;
                span.seal(text);
                spans.push(span);
                trailing = true;
            }
        } else {
            if trailing {
                flush_plain(&mut blocks, &mut spans);
                trailing = false;
            }
            if !spans.is_empty() {
                spans.push(Span::plain("\n"));
            }
            let mut span = Span::plain(line);
            span.range = offset as u32..(offset + line.len()) as u32;
            span.seal(text);
            spans.push(span);
        }
        offset += raw.len() + 1;
    }
    flush_plain(&mut blocks, &mut spans);
    Document {
        blocks,
        source: Arc::from(text),
        plain_file: true,
        ..Document::default()
    }
}

fn flush_plain(blocks: &mut Vec<Block>, spans: &mut Vec<Span>) {
    if spans.is_empty() {
        return;
    }
    let spans = std::mem::take(spans);
    let with_range: Vec<_> = spans.iter().filter(|s| !s.range.is_empty()).collect();
    let range = match (with_range.first(), with_range.last()) {
        (Some(first), Some(last)) => first.range.start as usize..last.range.end as usize,
        _ => 0..0,
    };
    let mut block = Block::plain(BlockKind::Paragraph { spans });
    block.range = range;
    blocks.push(block);
}

/// Extensionless file names with a known language, matched exactly
/// against the file name. `detect` consults it before anything else, so
/// a `Dockerfile` or a `Makefile` opens colored instead of falling
/// through to the content sniff. A suffixed variant (`Dockerfile.dev`)
/// stays unknown and still opens as plain code through the sniff.
static WELL_KNOWN_NAMES: &[(&str, &str)] = &[
    (".bash_login", "sh"),
    (".bash_logout", "sh"),
    (".bash_profile", "sh"),
    (".bashrc", "sh"),
    (".gitconfig", "ini"),
    (".gitmodules", "ini"),
    (".npmrc", "ini"),
    (".profile", "sh"),
    (".zlogin", "sh"),
    (".zprofile", "sh"),
    (".zshenv", "sh"),
    (".zshrc", "sh"),
    ("Containerfile", "dockerfile"),
    ("Dockerfile", "dockerfile"),
    ("GNUmakefile", "makefile"),
    ("Jenkinsfile", "groovy"),
    ("Makefile", "makefile"),
    ("PKGBUILD", "sh"),
    ("makefile", "makefile"),
];

/// Extension to highlight token, sorted by extension for binary search.
///
/// The mapping follows the bundled grammars' own extension lists, with one
/// class of exception: an extension a grammar claims that the wider world
/// reads as another language is left out, so `.s` stays assembly rather
/// than R and `.l` stays lex rather than Lisp. `.p`, `.t`, `.inc`, `.tmpl`,
/// `.tpl` and `.build` are dropped for the same reason.
///
/// Every token here reaches a grammar, some through `highlight::ALIASES`
/// where the grammar ships under another name.
static CODE_EXTENSIONS: &[(&str, &str)] = &[
    ("applescript", "applescript"),
    ("as", "actionscript"),
    ("bash", "bash"),
    ("bat", "batch"),
    ("bib", "bibtex"),
    ("c", "c"),
    ("cc", "cpp"),
    ("cfg", "ini"),
    ("cl", "lisp"),
    ("clj", "clojure"),
    ("cls", "latex"),
    ("cmd", "batch"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("css", "css"),
    ("csx", "csharp"),
    ("cxx", "cpp"),
    ("d", "d"),
    ("ddl", "sql"),
    ("di", "d"),
    ("diff", "diff"),
    ("dml", "sql"),
    ("dockerfile", "dockerfile"),
    ("dot", "graphviz"),
    ("dpr", "pascal"),
    ("el", "lisp"),
    ("erl", "erlang"),
    ("fish", "bash"),
    ("go", "go"),
    ("gql", "graphql"),
    ("gradle", "groovy"),
    ("graphql", "graphql"),
    ("groovy", "groovy"),
    ("gv", "graphviz"),
    ("gvy", "groovy"),
    ("h", "c"),
    ("hcl", "terraform"),
    ("hh", "cpp"),
    ("hpp", "cpp"),
    ("hrl", "erlang"),
    ("hs", "haskell"),
    ("htm", "html"),
    ("html", "html"),
    ("hxx", "cpp"),
    ("ini", "ini"),
    ("java", "java"),
    ("js", "javascript"),
    ("json", "json"),
    ("jsp", "jsp"),
    ("jsx", "javascript"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("lhs", "literate haskell"),
    ("lisp", "lisp"),
    ("ltx", "latex"),
    ("lua", "lua"),
    ("m", "objective-c"),
    ("mak", "makefile"),
    ("mjs", "javascript"),
    ("mk", "makefile"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("mm", "objective-c++"),
    ("opml", "xml"),
    ("pas", "pascal"),
    ("patch", "diff"),
    ("php", "php"),
    ("phtml", "php"),
    ("pl", "perl"),
    ("pm", "perl"),
    ("pod", "perl"),
    ("properties", "properties"),
    ("proto", "protobuf"),
    ("py", "python"),
    ("pyi", "python"),
    ("pyw", "python"),
    ("r", "r"),
    ("rb", "ruby"),
    ("rest", "rst"),
    ("rs", "rust"),
    ("rss", "xml"),
    ("rst", "rst"),
    ("sbt", "scala"),
    ("scala", "scala"),
    ("scm", "lisp"),
    ("sh", "bash"),
    ("sql", "sql"),
    ("sty", "latex"),
    ("swift", "swift"),
    ("tcl", "tcl"),
    ("tex", "latex"),
    ("textile", "textile"),
    ("tf", "terraform"),
    ("toml", "toml"),
    ("ts", "typescript"),
    ("tsx", "tsx"),
    ("xhtml", "html"),
    ("xml", "xml"),
    ("xsd", "xml"),
    ("xslt", "xml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
    ("zig", "zig"),
    ("zon", "zig"),
    ("zsh", "bash"),
];

/// The name of the temporary file a text piped into Oryx is written
/// to: `piped` with the extension the asked kind names (`--as md`), or
/// the one the text names by itself, or `txt`. The ordinary open path
/// takes it from there.
pub fn piped_name(kind: Option<&str>, text: &str) -> String {
    let extension = match kind {
        Some(kind) => kind.to_ascii_lowercase(),
        None => crate::style::highlight::sniff_extension(text)
            .unwrap_or("txt")
            .to_string(),
    };
    format!("piped.{extension}")
}

/// Whether a kind asked for on the command line can stand as a file
/// extension: letters, digits, and the few marks grammars use (`c++`,
/// `objective-c`, `c#`).
pub fn kind_is_plain(kind: &str) -> bool {
    (1..=32).contains(&kind.len())
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-#_".contains(c))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_piped_text_is_named_by_what_it_is() {
        use super::{detect, piped_name, FileKind};
        use std::path::Path;
        let diff = "diff --git a/src/app.rs b/src/app.rs\nindex 1d7918d..2323a24 100644\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,3 +1,4 @@\n fn main() {\n+    run();\n }\n";
        assert_eq!(piped_name(None, diff), "piped.diff");
        assert_eq!(
            piped_name(None, "{\n  \"name\": \"oryx\",\n  \"fast\": true\n}\n"),
            "piped.json"
        );
        assert_eq!(piped_name(None, "#!/bin/sh\necho hi\n"), "piped.sh");
        assert_eq!(
            piped_name(None, "#!/usr/bin/env python3\nprint(1)\n"),
            "piped.py"
        );
        assert_eq!(
            piped_name(None, "Just a few words.\nAnd a second line.\n"),
            "piped.txt"
        );
        assert_eq!(piped_name(None, ""), "piped.txt");
        assert_eq!(
            piped_name(Some("md"), diff),
            "piped.md",
            "the asked kind wins"
        );
        assert_eq!(piped_name(Some("RS"), "fn main() {}"), "piped.rs");
        assert_eq!(detect(Path::new("piped.md")), FileKind::Markdown);
        assert!(matches!(detect(Path::new("piped.diff")), FileKind::Code(_)));
        assert_eq!(detect(Path::new("piped.txt")), FileKind::Text);
    }

    #[test]
    fn a_kind_is_letters_digits_and_a_few_marks() {
        use super::kind_is_plain;
        for kind in ["md", "diff", "c++", "objective-c", "c#", "f90", "x_y"] {
            assert!(kind_is_plain(kind), "{kind}");
        }
        for kind in [
            "",
            "../x",
            "a/b",
            "a b",
            "a.b",
            "md\n",
            "x".repeat(40).as_str(),
        ] {
            assert!(!kind_is_plain(kind), "{kind:?}");
        }
    }

    use super::*;
    use crate::doc::model::BlockKind;
    use std::path::PathBuf;

    fn detect_ext(name: &str) -> FileKind {
        detect(Path::new(name))
    }

    /// A Quartz-produced PDF opens with an all-ASCII head, so the NUL
    /// sniff alone cannot catch it; the extension must.
    #[test]
    fn pdf_is_known_and_refused_before_the_sniff() {
        assert_eq!(detect_ext("book.pdf"), FileKind::Undisplayable);
        assert_eq!(detect_ext("BOOK.PDF"), FileKind::Undisplayable);

        let dir = std::env::temp_dir().join(format!("oryx-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ascii-head.pdf");
        std::fs::write(
            &path,
            b"%PDF-1.4\n1 0 obj\n<< /Producer (Quartz PDFContext) >>\nendobj\n",
        )
        .unwrap();
        assert!(
            !is_text_file(&path),
            "an ASCII-headed PDF must not sniff as text"
        );
        let err = match open(&path, None) {
            Err(err) => err,
            Ok(_) => panic!("an ASCII-headed PDF must refuse"),
        };
        assert!(err.to_string().contains("PDF"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_text_file_is_one_block_of_prose_lines() {
        let dir = std::env::temp_dir().join(format!("oryx-textdoc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, "alpha\n\nbeta\n\n").unwrap();
        let d = open(&path, None).unwrap().document;
        std::fs::remove_dir_all(&dir).ok();
        assert!(d.plain_file, "a text file is a plain file");
        assert!(!d.code_file, "a text file keeps the page background");
        assert_eq!(d.blocks.len(), 1, "the whole file is one block");
        let BlockKind::CodeBlock {
            language, lines, ..
        } = &d.blocks[0].kind
        else {
            panic!("a text file is line-oriented");
        };
        assert!(language.is_none());
        assert_eq!(
            lines.len(),
            4,
            "every line is a row, the trailing blank included"
        );
    }

    #[test]
    fn a_code_document_keeps_its_trailing_empty_lines() {
        let rows = |token: &str, text: &str| match &code_document(Some(token), text).blocks[0].kind
        {
            BlockKind::CodeBlock { lines, .. } => lines.len(),
            _ => unreachable!(),
        };
        assert_eq!(rows("rs", "a\nb\n\n\n"), 4, "two empty lines, two rows");
        assert_eq!(
            rows("rs", "a\nb\n"),
            2,
            "the final newline ends the last row and opens none"
        );
        assert_eq!(rows("rs", "a\nb"), 2);
        assert_eq!(rows("rs", ""), 0);
        assert_eq!(
            rows("md", "# t\n\n\n"),
            3,
            "the source view of a markdown file too"
        );
    }

    #[test]
    fn a_file_ending_in_blank_lines_round_trips() {
        let dir = std::env::temp_dir().join("oryx_trailing_blank_lines");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tail.rs");
        std::fs::write(&path, "a\n\n\n").unwrap();
        let opened = open(&path, None).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let BlockKind::CodeBlock { lines, .. } = &opened.document.blocks[0].kind else {
            panic!("a code file");
        };
        assert_eq!(
            lines.len(),
            3,
            "a row per line, the two empty ones included"
        );
        let led = crate::edit::splice::Ledger::new(opened.document.source.clone(), opened.crlf);
        assert_eq!(led.emit(), b"a\n\n\n", "the bytes come back untouched");
    }

    #[test]
    fn crlf_positions_are_recorded_for_the_ledger() {
        let dir = std::env::temp_dir().join(format!("oryx-crlf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mixed = dir.join("mixed.txt");
        std::fs::write(&mixed, b"alpha\r\nbeta\ngamma\r\n").unwrap();
        let opened = open(&mixed, None).unwrap();
        assert_eq!(&*opened.document.source, "alpha\nbeta\ngamma\n");
        assert_eq!(
            opened.crlf,
            vec![5, 16],
            "each normalized newline is on record at its text offset"
        );
        let clean = dir.join("clean.txt");
        std::fs::write(&clean, "alpha\nbeta\n").unwrap();
        let opened = open(&clean, None).unwrap();
        assert!(opened.crlf.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A damaged file ends its lines in CR CR LF, an old Mac and a
    /// Windows conversion on top of each other. Every return before a
    /// line break leaves the text, so the editor's line math sees one
    /// byte per line break, and each is on record so the save writes
    /// the line back as it was.
    #[test]
    fn stray_returns_before_a_line_break_leave_the_text_and_go_on_record() {
        let dir = std::env::temp_dir().join(format!("oryx-crcrlf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("damaged.htm");
        let bytes = b"one\r\r\ntwo\r\nthree\n\r\r\n";
        std::fs::write(&path, bytes).unwrap();
        let opened = open(&path, None).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(&*opened.document.source, "one\ntwo\nthree\n\n");
        assert_eq!(
            opened.crlf,
            vec![3, 3, 7, 14, 14],
            "one entry per return, at the offset of the line break it stood before"
        );
        let led = crate::edit::splice::Ledger::new(opened.document.source.clone(), opened.crlf);
        assert_eq!(led.emit(), bytes, "the bytes come back untouched");
    }

    /// A classic Mac OS file breaks its lines with CR alone and has no
    /// LF at all. It reads as lines, is marked, and saves with CR again.
    #[test]
    fn a_file_of_cr_line_breaks_reads_as_lines_and_is_marked() {
        let dir = std::env::temp_dir().join(format!("oryx-cr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mac.txt");
        let bytes = b"one\rtwo\r\rfour\r";
        std::fs::write(&path, bytes).unwrap();
        let opened = open(&path, None).unwrap();
        assert_eq!(&*opened.document.source, "one\ntwo\n\nfour\n");
        assert!(opened.cr, "the file's line break is CR");
        assert!(opened.crlf.is_empty());
        let led = crate::edit::splice::Ledger::new(opened.document.source.clone(), opened.crlf)
            .with_cr(opened.cr);
        assert_eq!(led.emit(), bytes, "the bytes come back untouched");
        // A file with any LF keeps its lone returns as text: only a file
        // with no LF at all is a CR file.
        let mixed = dir.join("mixed.txt");
        std::fs::write(&mixed, b"one\rtwo\nthree\n").unwrap();
        let opened = open(&mixed, None).unwrap();
        assert_eq!(&*opened.document.source, "one\rtwo\nthree\n");
        assert!(!opened.cr);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pasted_text_loses_its_returns_before_line_breaks_only() {
        assert_eq!(without_returns("a\r\nb\r\r\nc\n"), "a\nb\nc\n");
        assert_eq!(without_returns("a\rb\n"), "a\rb\n", "a lone return is text");
        assert_eq!(without_returns("tail\r"), "tail\r", "no break, no strip");
        assert_eq!(without_returns(""), "");
    }

    #[test]
    fn a_byte_order_mark_is_stripped_and_recorded() {
        let dir = std::env::temp_dir().join(format!("oryx-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marked = dir.join("marked.md");
        std::fs::write(&marked, b"\xEF\xBB\xBF# Title\r\n\r\nbody\r\n").unwrap();
        let opened = open(&marked, None).unwrap();
        assert!(opened.bom, "the mark is on record");
        assert!(
            !opened.document.source.starts_with('\u{FEFF}'),
            "the mark left the text"
        );
        assert!(
            matches!(opened.document.blocks[0].kind, BlockKind::Heading { .. }),
            "the first heading parses: {:?}",
            opened.document.blocks[0].kind
        );
        assert_eq!(
            opened.crlf,
            vec![7, 8, 13],
            "CRLF offsets count from the stripped text"
        );
        let code = dir.join("marked.rs");
        std::fs::write(&code, b"\xEF\xBB\xBFfn main() {}\n").unwrap();
        let opened = open(&code, None).unwrap();
        assert!(opened.bom);
        assert_eq!(&*opened.document.source, "fn main() {}\n");
        let clean = dir.join("clean.md");
        std::fs::write(&clean, "# Title\n").unwrap();
        assert!(
            !open(&clean, None).unwrap().bom,
            "a clean file carries no mark"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_lossy_decode_is_recorded_and_a_clean_one_is_not() {
        let dir = std::env::temp_dir().join(format!("oryx-lossy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let latin = dir.join("latin.txt");
        std::fs::write(&latin, b"caf\xE9 au lait\n").unwrap();
        let opened = open(&latin, None).unwrap();
        assert!(opened.lossy, "an invalid byte forces the lossy read");
        assert!(opened.document.source.contains('\u{FFFD}'));
        let clean = dir.join("clean.txt");
        std::fs::write(&clean, "café au lait\n").unwrap();
        let opened = open(&clean, None).unwrap();
        assert!(!opened.lossy, "a clean file is not branded lossy");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crlf_markdown_normalizes_at_load() {
        let dir = std::env::temp_dir().join(format!("oryx-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crlf.md");
        std::fs::write(&path, "# Title\r\n\r\nA body line.\r\n").unwrap();
        let opened = open(&path, None).unwrap();
        assert!(
            !opened.document.source.contains('\r'),
            "the source is clean"
        );
        let twin = crate::doc::markdown::parse("# Title\n\nA body line.\n");
        assert_eq!(opened.document.blocks.len(), twin.blocks.len());
        assert_eq!(opened.document.source, twin.source);
    }

    #[test]
    fn markdown_extensions() {
        assert_eq!(detect_ext("a.md"), FileKind::Markdown);
        assert_eq!(detect_ext("b.markdown"), FileKind::Markdown);
        assert_eq!(detect_ext("UPPER.MD"), FileKind::Markdown);
    }

    #[test]
    fn code_extensions_map_to_tokens() {
        for (name, token) in [
            ("m.rs", "rust"),
            ("m.py", "python"),
            ("m.js", "javascript"),
            ("m.ts", "typescript"),
            ("m.go", "go"),
            ("m.java", "java"),
            ("m.c", "c"),
            ("m.cpp", "cpp"),
            ("m.rb", "ruby"),
            ("m.sh", "bash"),
            ("m.yaml", "yaml"),
            ("m.yml", "yaml"),
            ("m.toml", "toml"),
            ("m.json", "json"),
            ("m.html", "html"),
            ("m.css", "css"),
            ("m.sql", "sql"),
            ("m.kts", "kotlin"),
            ("m.tsx", "tsx"),
            ("m.zig", "zig"),
            ("m.zon", "zig"),
            ("m.tf", "terraform"),
            ("m.hcl", "terraform"),
            ("m.graphql", "graphql"),
            ("m.gql", "graphql"),
            ("m.proto", "protobuf"),
            ("m.dockerfile", "dockerfile"),
        ] {
            assert_eq!(detect_ext(name), FileKind::Code(token), "{name}");
        }
    }

    #[test]
    fn unknown_and_missing_extensions_are_unknown() {
        assert_eq!(detect_ext("notes.xyz"), FileKind::Unknown);
        assert_eq!(detect_ext("README"), FileKind::Unknown);
        assert_eq!(detect_ext("LICENSE"), FileKind::Unknown);
        assert_eq!(detect_ext("archive.txt"), FileKind::Text);
    }

    #[test]
    fn well_known_names_map_to_tokens() {
        for (name, token) in [
            ("Dockerfile", "dockerfile"),
            ("Containerfile", "dockerfile"),
            ("Makefile", "makefile"),
            ("makefile", "makefile"),
            ("GNUmakefile", "makefile"),
        ] {
            assert_eq!(detect_ext(name), FileKind::Code(token), "{name}");
        }
        let nested = PathBuf::from("dir/Makefile");
        assert_eq!(detect(&nested), FileKind::Code("makefile"));
        assert_eq!(detect_ext("Dockerfile.dev"), FileKind::Unknown);
    }

    #[test]
    fn the_extension_table_is_sorted_and_free_of_duplicates() {
        for pair in CODE_EXTENSIONS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for pair in WELL_KNOWN_NAMES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn every_extension_reaches_a_grammar() {
        for (ext, token) in CODE_EXTENSIONS {
            assert!(
                highlight::grammar_of(token).is_some(),
                ".{ext} maps to {token}, which no grammar answers"
            );
        }
        for (name, token) in WELL_KNOWN_NAMES {
            assert!(
                highlight::grammar_of(token).is_some(),
                "{name} maps to {token}, which no grammar answers"
            );
        }
    }

    #[test]
    fn extensions_reach_the_expected_grammar() {
        for (name, grammar) in [
            ("a.hs", "Haskell"),
            ("a.lhs", "Literate Haskell"),
            ("a.m", "Objective-C"),
            ("a.mm", "Objective-C++"),
            ("a.tex", "LaTeX"),
            ("a.bat", "Batch File"),
            ("a.dot", "Graphviz (DOT)"),
            ("a.rst", "reStructuredText"),
            ("a.scala", "Scala"),
            ("a.erl", "Erlang"),
            ("a.clj", "Clojure"),
            ("a.ml", "OCaml"),
            ("a.mk", "Makefile"),
            ("a.diff", "Diff"),
            ("a.properties", "Java Properties"),
            ("a.r", "R"),
            ("a.tcl", "Tcl"),
            ("a.groovy", "Groovy"),
            ("a.pas", "Pascal"),
            ("a.htm", "HTML"),
            ("a.cxx", "C++"),
            ("a.pyw", "Python"),
            ("a.toml", "TOML"),
            ("a.ini", "INI"),
            ("a.cfg", "INI"),
            ("a.kt", "Kotlin"),
            ("a.kts", "Kotlin"),
            ("a.swift", "Swift"),
            ("a.ts", "TypeScript"),
            ("a.tsx", "TypeScriptReact"),
            ("a.zig", "Zig"),
            ("a.tf", "Terraform"),
            ("a.hcl", "Terraform"),
            ("a.graphql", "GraphQL"),
            ("a.proto", "Protocol Buffer"),
            ("a.dockerfile", "Containerfile"),
            ("Dockerfile", "Containerfile"),
            ("Makefile", "Makefile"),
        ] {
            let FileKind::Code(token) = detect_ext(name) else {
                panic!("{name} is not code")
            };
            assert_eq!(highlight::grammar_of(token), Some(grammar), "{name}");
        }
    }

    #[test]
    fn extensions_the_wider_world_reads_differently_are_left_out() {
        for name in [
            "boot.s",
            "scan.l",
            "prog.p",
            "case.t",
            "part.inc",
            "page.tmpl",
            "site.tpl",
            "x.build",
        ] {
            assert_eq!(detect_ext(name), FileKind::Unknown, "{name}");
        }
    }

    #[test]
    fn a_nul_byte_in_the_sniff_window_marks_binary() {
        assert!(is_binary(b"\x7fELF\x02\x01\x01\x00"));
        assert!(!is_binary(
            "h\u{e9}llo w\u{f6}rld\nsecond line\n".as_bytes()
        ));
        assert!(!is_binary(b""));
        let mut late = vec![b'a'; SNIFF + 16];
        late[SNIFF + 8] = 0;
        assert!(!is_binary(&late), "a NUL past the window does not count");
    }

    /// A ROM whose first zero byte sits past the window: its head is
    /// unreadable bytes from end to end, which no text file is. A
    /// heavily accented Latin-1 file, a log full of escape codes and a
    /// window cut inside a character all stay text.
    #[test]
    fn a_head_of_unreadable_bytes_marks_binary_without_a_nul() {
        let mut rom = vec![0xFFu8; SNIFF];
        rom.extend_from_slice(b"\x00\x00");
        assert!(
            is_binary(&rom),
            "all high bytes, the first zero past the window"
        );
        let mut code = Vec::new();
        for i in 0..SNIFF {
            code.push((i * 7 % 250) as u8 + 1);
        }
        assert!(
            is_binary(&code),
            "machine code without a zero in the window"
        );
        let latin = b"caf\xe9 au lait, na\xefve r\xe9sum\xe9, cr\xe8me br\xfbl\xe9e.\n".repeat(200);
        assert!(!is_binary(&latin), "Latin-1 text reads lossy, not binary");
        let log = b"\x1b[31mERROR\x1b[0m something failed\n\x1b[32mOK\x1b[0m\n".repeat(300);
        assert!(!is_binary(&log), "escape codes are text");
        let mut cut = "\u{e9}".repeat(SNIFF).into_bytes();
        cut.truncate(SNIFF);
        assert!(
            !is_binary(&cut),
            "the window cutting a character is no fault"
        );
        let mostly_control: Vec<u8> = (0..SNIFF)
            .map(|i| if i % 2 == 0 { 1 } else { b'a' })
            .collect();
        assert!(is_binary(&mostly_control), "half control characters");
    }

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("oryx-load-{}-{name}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn code_file_becomes_one_code_block() {
        let path = temp_file("t.py", "def f():\n    return 1\n");
        let d = open(&path, None).unwrap().document;
        std::fs::remove_file(&path).unwrap();
        let BlockKind::CodeBlock {
            language,
            lines,
            highlights,
            ..
        } = &d.blocks[0].kind
        else {
            panic!("expected code block, got {:?}", d.blocks)
        };
        assert_eq!(language.as_deref(), Some("python"));
        assert_eq!(
            lines.iter(&d.source).collect::<Vec<_>>(),
            ["def f():", "    return 1"]
        );
        assert_eq!(highlights.len(), 2);
    }

    #[test]
    fn past_deadline_leaves_code_pending() {
        let path = temp_file(
            "t2.md",
            "# T\n\n```rust\nfn a() {}\n```\n\ntext\n\n```python\nx = 1\n```\n",
        );
        let opened = open(&path, Some(Instant::now())).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(opened.pending.len(), 2);
        assert_eq!(opened.pending[0].language.as_deref(), Some("rust"));
        assert_eq!(opened.pending[1].language.as_deref(), Some("python"));
        for p in &opened.pending {
            let BlockKind::CodeBlock {
                lines, highlights, ..
            } = &opened.document.blocks[p.block].kind
            else {
                panic!("pending index does not point at a code block")
            };
            assert_eq!(&p.lines, lines);
            assert!(highlights.is_empty());
        }
    }

    #[test]
    fn no_deadline_matches_eager_highlighting() {
        let path = temp_file("t3.rs", "fn main() {\n    let x = 1;\n}\n");
        let opened = open(&path, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(opened.pending.is_empty());
        let BlockKind::CodeBlock {
            lines, highlights, ..
        } = &opened.document.blocks[0].kind
        else {
            panic!("expected code block")
        };
        assert_eq!(
            highlights,
            &highlight::spans(&opened.document.source, lines, Some("rust"))
        );
        assert!(highlights
            .iter()
            .flatten()
            .any(|(_, role)| *role != crate::style::highlight::SyntaxRole::Plain));
    }

    #[test]
    fn fold_grows_and_overwrites_the_prefix() {
        let path = temp_file("t4.rs", "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n");
        let mut opened = open(&path, Some(Instant::now())).unwrap();
        std::fs::remove_file(&path).unwrap();
        let eager = {
            let BlockKind::CodeBlock { lines, .. } = &opened.document.blocks[0].kind else {
                panic!("expected code block")
            };
            highlight::spans(&opened.document.source, lines, Some("rust"))
        };
        fold(
            &mut opened.document,
            &Arrival {
                block: 0,
                start_line: 0,
                spans: eager[0..2].to_vec(),
                seam: None,
                converged: false,
                reach: 0,
                speculative: false,
            },
        );
        let BlockKind::CodeBlock { highlights, .. } = &opened.document.blocks[0].kind else {
            panic!()
        };
        assert_eq!(highlights.len(), 2);
        fold(
            &mut opened.document,
            &Arrival {
                block: 0,
                start_line: 2,
                spans: eager[2..4].to_vec(),
                seam: None,
                converged: false,
                reach: 0,
                speculative: false,
            },
        );
        let BlockKind::CodeBlock { highlights, .. } = &opened.document.blocks[0].kind else {
            panic!()
        };
        assert_eq!(highlights, &eager);
    }

    #[test]
    fn the_exact_frontier_follows_folds() {
        let text: String = (0..1200).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut doc = code_document(Some("rs"), &text);
        let exact_of = |doc: &Document| match &doc.blocks[0].kind {
            BlockKind::CodeBlock { exact, .. } => *exact,
            _ => unreachable!(),
        };
        let arrival = |start: usize, n: usize, converged: bool, speculative: bool| Arrival {
            block: 0,
            start_line: start,
            spans: vec![Vec::new(); n],
            seam: None,
            converged,
            // The table of a complete sweep reaches the block's end.
            reach: if converged { 1200 } else { 0 },
            speculative,
        };
        fold(&mut doc, &arrival(0, 512, false, false));
        assert_eq!(
            exact_of(&doc),
            512,
            "a contiguous fold advances the frontier"
        );
        fold(&mut doc, &arrival(1100, 100, false, true));
        assert_eq!(exact_of(&doc), 512, "a speculative fold never advances it");
        assert!(
            !pending(&doc).is_empty(),
            "the block still wants its wash though its rows reach the end"
        );
        fold(&mut doc, &arrival(600, 100, false, false));
        assert_eq!(exact_of(&doc), 512, "a gapped fold does not advance it");
        fold(&mut doc, &arrival(512, 188, false, false));
        assert_eq!(
            exact_of(&doc),
            700,
            "the sweep advances through delivered rows"
        );
        fold(&mut doc, &arrival(700, 100, true, false));
        assert_eq!(
            exact_of(&doc),
            1200,
            "a converged stop restores the frontier to the end"
        );
        assert!(pending(&doc).is_empty(), "the wash is complete");
    }

    /// A converged stop against a table that stops short of the
    /// block's end, as a keystroke during the first sweep leaves it:
    /// the frontier moves to the table's reach, no further, and the
    /// chunks the sweep goes on with advance it from there.
    #[test]
    fn a_converged_stop_moves_the_frontier_to_the_table_s_reach() {
        let text: String = (0..3000).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut doc = code_document(Some("rs"), &text);
        let exact_of = |doc: &Document| match &doc.blocks[0].kind {
            BlockKind::CodeBlock { exact, .. } => *exact,
            _ => unreachable!(),
        };
        let arrival = |start: usize, n: usize, converged: bool, reach: usize| Arrival {
            block: 0,
            start_line: start,
            spans: vec![Vec::new(); n],
            seam: None,
            converged,
            reach,
            speculative: false,
        };
        fold(&mut doc, &arrival(0, 512, true, 1536));
        assert_eq!(
            exact_of(&doc),
            1536,
            "the stop vouches up to the table's reach"
        );
        assert!(!pending(&doc).is_empty(), "the tail still wants its wash");
        fold(&mut doc, &arrival(1536, 512, false, 0));
        assert_eq!(
            exact_of(&doc),
            2048,
            "the chunks after it advance the frontier"
        );
    }

    /// The seam table of a complete sweep of a document's first block,
    /// as the worker files it.
    fn table_of(doc: &Document) -> Vec<(usize, Seam)> {
        let BlockKind::CodeBlock {
            language, lines, ..
        } = &doc.blocks[0].kind
        else {
            unreachable!()
        };
        let mut table = Vec::new();
        highlight::spans_chunked(
            &doc.source,
            lines,
            language.as_deref(),
            highlight::CHUNK_LINES,
            None,
            || true,
            |c| {
                highlight::record_seam(&mut table, c.start_line + c.spans.len(), &c.seam);
                true
            },
        );
        table
    }

    fn set_exact(doc: &mut Document, block: usize, to: usize) {
        if let BlockKind::CodeBlock { exact, .. } = &mut doc.blocks[block].kind {
            *exact = to;
        }
    }

    /// The resume of a block still to color, from its stored table:
    /// the sweep starts at the seam at or before the frontier and
    /// converges against the seams past it, and past the burst's end
    /// when a later edit of the burst stands beyond the first of them,
    /// so the lines between the edits are colored again.
    #[test]
    fn a_resume_converges_only_past_the_burst_s_end() {
        let text: String = (0..3000).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut doc = code_document(Some("rs"), &text);
        let table = table_of(&doc);
        assert_eq!(
            table.iter().map(|s| s.0).collect::<Vec<_>>(),
            [512, 1024, 1536, 2048, 2560, 3000]
        );
        let seams = HashMap::from([(0, table)]);
        let first_expected = |after: usize, doc: &Document| {
            let pending = pending_from(doc, &seams, after, &[]);
            assert_eq!(pending.len(), 1, "one block still to color");
            let resume = pending[0].resume.clone().expect("a table gives a resume");
            (
                resume.start_line,
                resume.seam.is_some(),
                resume.expected.first().map(|s| s.0),
            )
        };
        set_exact(&mut doc, 0, 100);
        assert_eq!(
            first_expected(0, &doc),
            (0, false, Some(512)),
            "no burst: from line 0, the first seam past the frontier"
        );
        assert_eq!(
            first_expected(1200, &doc),
            (0, false, Some(1536)),
            "a burst to line 1200: the first seam at or past it"
        );
        assert_eq!(
            first_expected(1024, &doc),
            (0, false, Some(1024)),
            "a burst ending on a seam's line keeps that seam"
        );
        set_exact(&mut doc, 0, 600);
        assert_eq!(
            first_expected(0, &doc),
            (512, true, Some(1024)),
            "a frontier past a seam starts from that seam"
        );
        set_exact(&mut doc, 0, 3000);
        assert!(
            pending_from(&doc, &seams, 0, &[]).is_empty(),
            "a complete block is not listed"
        );
        set_exact(&mut doc, 0, 100);
        assert!(
            pending_from(&doc, &HashMap::new(), 0, &[])[0]
                .resume
                .is_none(),
            "a block without a table sweeps from line 0"
        );
    }

    /// The cuts a resume carries: the lines of the last key, for the
    /// first block alone, since a burst is the one block of a code file
    /// or of the source view.
    #[test]
    fn the_resume_cuts_at_the_lines_of_the_last_key() {
        let text: String = (0..1500).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut doc = code_document(Some("rs"), &text);
        let seams = HashMap::from([(0, table_of(&doc))]);
        set_exact(&mut doc, 0, 600);
        let pending = pending_from(&doc, &seams, 601, &[600, 601]);
        assert_eq!(
            pending[0].resume.as_ref().map(|r| r.cuts.clone()),
            Some(vec![600, 601])
        );
        let doc =
            crate::doc::markdown::parse("```rust\nlet a = 1;\n```\n\n```rust\nlet b = 2;\n```\n");
        let seams = HashMap::from([(0, table_of(&doc)), (1, Vec::new())]);
        let pending = pending_from(&doc, &seams, 0, &[0, 1]);
        assert!(
            pending
                .iter()
                .all(|p| p.resume.as_ref().is_none_or(|r| r.cuts.is_empty())),
            "a page's blocks get no cuts"
        );
    }

    /// A page's blocks: every block still to color is listed, with a
    /// resume where it has a table, from line 0 where it has none, and
    /// a complete block is not.
    #[test]
    fn every_block_still_to_color_is_listed_with_its_own_resume() {
        let mut doc =
            crate::doc::markdown::parse("```rust\nlet a = 1;\n```\n\n```rust\nlet b = 2;\n```\n");
        assert_eq!(doc.blocks.len(), 2);
        let seams = HashMap::from([(1, table_of(&doc))]);
        let pending = pending_from(&doc, &seams, 0, &[]);
        assert_eq!(pending.len(), 2, "both cold");
        assert!(pending[0].resume.is_none() && pending[1].resume.is_some());
        set_exact(&mut doc, 0, 1);
        let pending = pending_from(&doc, &seams, 0, &[]);
        assert_eq!(pending.len(), 1, "the first is complete");
        assert_eq!(pending[0].block, 1);
    }

    #[test]
    fn a_late_guess_leaves_the_exact_lines_alone() {
        use crate::style::highlight::SyntaxRole;
        let text: String = (0..1200).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut doc = code_document(Some("rs"), &text);
        let colors = |doc: &Document, line: usize| match &doc.blocks[0].kind {
            BlockKind::CodeBlock { highlights, .. } => highlights[line].clone(),
            _ => unreachable!(),
        };
        let exact = vec![(0..3, SyntaxRole::Keyword)];
        let guessed = vec![(0..3, SyntaxRole::Comment)];
        let arrival = |start: usize,
                       n: usize,
                       spans: &Vec<(std::ops::Range<usize>, SyntaxRole)>,
                       speculative: bool| Arrival {
            block: 0,
            start_line: start,
            spans: vec![spans.clone(); n],
            seam: None,
            converged: false,
            reach: 0,
            speculative,
        };
        // The exact sweep colored the first 512 lines; a guess asked
        // before it reached them lands after it, for lines 500 to 600.
        fold(&mut doc, &arrival(0, 512, &exact, false));
        fold(&mut doc, &arrival(500, 100, &guessed, true));
        assert_eq!(colors(&doc, 505), exact, "an exact line keeps its colors");
        assert_eq!(colors(&doc, 550), guessed, "a cold line takes the guess");
    }

    #[test]
    fn an_over_target_markdown_opens_with_a_prefix_over_the_full_source() {
        let para = "A paragraph of plain filler text for the streaming fixture.\n\n";
        let source = para.repeat(2 + crate::doc::stream::PREFIX_TARGET / para.len());
        let path = temp_file("stream.md", &source);
        let opened = open(&path, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(opened.streamed, "an over-target markdown streams");
        assert_eq!(&*opened.document.source, source, "the source is whole");
        let full = crate::doc::markdown::parse(source.as_str());
        let n = opened.document.blocks.len();
        assert!(n > 0 && n < full.blocks.len(), "the blocks cover a prefix");
        assert_eq!(opened.document.blocks, full.blocks[..n], "the head matches");
    }

    #[test]
    fn an_under_target_markdown_takes_the_sync_path() {
        let path = temp_file("sync.md", "# Title\n\na paragraph\n");
        let opened = open(&path, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!opened.streamed);
        let full = crate::doc::markdown::parse("# Title\n\na paragraph\n");
        assert_eq!(opened.document.blocks, full.blocks);
    }

    #[test]
    fn a_code_file_never_streams() {
        let line = "let value = compute(input); // filler\n";
        let source = line.repeat(2 + crate::doc::stream::PREFIX_TARGET / line.len());
        let path = temp_file("stream.rs", &source);
        let opened = open(&path, Some(Instant::now())).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!opened.streamed);
        assert_eq!(opened.document.blocks.len(), 1, "one block, whole file");
    }

    #[test]
    fn pending_lists_only_incomplete_code_blocks() {
        let mut doc = crate::doc::markdown::parse(
            "```rust\nfn a() {}\n```\n\ntext\n\n```python\nx = 1\ny = 2\n```\n",
        );
        let source = Arc::clone(&doc.source);
        let BlockKind::CodeBlock {
            lines,
            highlights,
            exact,
            ..
        } = &mut doc.blocks[0].kind
        else {
            panic!("expected a code block")
        };
        *highlights = highlight::spans(&source, lines, Some("rust"));
        *exact = highlights.len();
        let pending = pending(&doc);
        assert_eq!(pending.len(), 1, "the complete block is not redone");
        assert_eq!(pending[0].block, 2);
        assert_eq!(pending[0].language.as_deref(), Some("python"));
        assert_eq!(
            pending[0]
                .lines
                .iter(&pending[0].source)
                .collect::<Vec<_>>(),
            ["x = 1", "y = 2"]
        );
    }

    #[test]
    fn a_message_document_splits_paragraphs_on_blank_lines() {
        let d = plain_document("line one\nline two\n\nsecond para\n");
        assert_eq!(d.blocks.len(), 2);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let joined: String = spans.iter().map(|s| s.text(&d.source)).collect();
        assert_eq!(
            joined, "line one\nline two\n\n",
            "the blank row rides the paragraph as its own line"
        );
        assert!(matches!(&d.blocks[1].kind, BlockKind::Paragraph { .. }));
    }

    #[test]
    fn an_unknown_text_file_becomes_one_code_block_with_no_language() {
        let path = temp_file("t.conf", "listen = 80\nroot = /srv\n");
        let d = open(&path, None).unwrap().document;
        std::fs::remove_file(&path).unwrap();
        assert_eq!(d.blocks.len(), 1);
        let BlockKind::CodeBlock {
            language, lines, ..
        } = &d.blocks[0].kind
        else {
            panic!("expected a code block")
        };
        assert_eq!(*language, None);
        assert_eq!(
            lines.iter(&d.source).collect::<Vec<_>>(),
            ["listen = 80", "root = /srv"]
        );
    }

    #[test]
    fn an_extensionless_text_file_opens_as_code() {
        let path = temp_file("Makefile", "all:\n\tcargo build\n");
        let d = open(&path, None).unwrap().document;
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(&d.blocks[0].kind, BlockKind::CodeBlock { .. }));
    }

    #[test]
    fn a_text_in_an_old_encoding_is_refused_as_not_utf8() {
        // Russian prose in CP1251: every letter is a high byte, no zero
        // byte anywhere. It is text, in an encoding Oryx does not read.
        let line = b"\xc2\xf1\xe5 \xf1\xf7\xe0\xf1\xf2\xeb\xe8\xe2\xfb\xe5 \xf1\xe5\xec\xfc\xe8.\n";
        let path =
            std::env::temp_dir().join(format!("oryx-load-{}-cp1251.txt", std::process::id()));
        std::fs::write(&path, line.repeat(40)).unwrap();
        let Err(err) = open(&path, None) else {
            panic!("a CP1251 file opened")
        };
        let err = err.to_string();
        std::fs::remove_file(&path).unwrap();
        assert!(err.ends_with("cp1251.txt is not UTF-8 text"), "{err}");
    }

    #[test]
    fn a_binary_file_is_refused_by_name() {
        let path = std::env::temp_dir().join(format!("oryx-load-{}-t.bin", std::process::id()));
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").unwrap();
        let Err(err) = open(&path, None) else {
            panic!("a binary file opened")
        };
        let err = err.to_string();
        std::fs::remove_file(&path).unwrap();
        assert!(err.contains("not a text file"), "{err}");
        assert!(err.contains("t.bin"), "{err}");
    }

    #[test]
    fn markdown_file_parses_as_markdown() {
        let path = temp_file("t.md", "# Title\n\nbody\n");
        let d = open(&path, None).unwrap().document;
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(&d.blocks[0].kind, BlockKind::Heading { .. }));
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(open(Path::new("/nonexistent/oryx-missing.md"), None).is_err());
    }

    #[test]
    fn message_becomes_a_plain_document() {
        let d = message("cannot open /x: denied");
        assert_eq!(d.blocks.len(), 1);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!("expected a paragraph")
        };
        assert_eq!(spans[0].text(&d.source), "cannot open /x: denied");
    }

    #[test]
    fn recognized_extensions_cover_the_renderable_set() {
        let exts = recognized_extensions();
        for e in ["md", "markdown", "txt", "rs", "py", "toml", "yaml"] {
            assert!(exts.contains(&e), "{e} missing");
        }
    }

    /// The common shell and git files without an extension are named in
    /// the table, since the grammars know only some of them.
    #[test]
    fn well_known_dotfiles_have_their_token() {
        for (name, token) in [
            (".zshrc", "sh"),
            (".zshenv", "sh"),
            (".bash_profile", "sh"),
            (".profile", "sh"),
            ("PKGBUILD", "sh"),
            (".gitconfig", "ini"),
            (".gitmodules", "ini"),
            ("Jenkinsfile", "groovy"),
        ] {
            assert_eq!(detect(Path::new(name)), FileKind::Code(token), "{name}");
        }
    }

    /// Opened, a file without an extension carries the grammar its text
    /// or its name gives away, and prose stays a plain code document.
    #[test]
    fn files_without_an_extension_open_with_their_colors() {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/noext"));
        let language = |name: &str| -> Option<String> {
            let opened = open(&dir.join(name), None).expect(name);
            match &opened.document.blocks[0].kind {
                BlockKind::CodeBlock { language, .. } => language.clone(),
                other => panic!("{name}: not a code document: {other:?}"),
            }
        };
        assert_eq!(language("script").as_deref(), Some("Python"));
        assert_eq!(language("modeline").as_deref(), Some("Ruby"));
        assert_eq!(language("emacs").as_deref(), Some("Lisp"));
        assert_eq!(language(".bashrc").as_deref(), Some("sh"));
        assert_eq!(language(".zshrc").as_deref(), Some("sh"));
        assert_eq!(language(".gitconfig").as_deref(), Some("ini"));
        assert_eq!(language("patch").as_deref(), Some("Diff"));
        assert_eq!(language("settings").as_deref(), Some("JSON"));
        assert_eq!(language("config").as_deref(), Some("INI"));
        assert_eq!(language("README"), None, "prose stays plain");
        assert_eq!(
            detect_with_text(&dir.join("script"), "#!/usr/bin/env python3\n"),
            FileKind::Code("Python"),
            "the kind with the text at hand, for the editor's round trip"
        );
    }
}

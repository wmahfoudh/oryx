//! File loading and type detection by extension.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::doc::model::{Block, BlockKind, CodeBody, Document, Span};
use crate::style::highlight::{self, Arrival, PendingBlock};

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
    /// A format Oryx knows it cannot display (PDF). Refused by name:
    /// some PDFs open with an all-ASCII head the content sniff passes.
    Undisplayable,
    /// Not identified by extension. Its content decides: text opens as code
    /// with no language, binary is refused.
    Unknown,
}

pub fn detect(path: &Path) -> FileKind {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Ok(i) = WELL_KNOWN_NAMES.binary_search_by_key(&name, |(k, _)| k) {
            return FileKind::Code(WELL_KNOWN_NAMES[i].1);
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
/// container and executable format has one in its header.
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(SNIFF)].contains(&0)
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
    pub book: Option<super::epub::BookJob>,
    /// A book's table of contents as authored; empty for files.
    pub toc: Vec<super::epub::TocEntry>,
    /// True when the bytes decoded only through lossy UTF-8
    /// replacement. Editing refuses such a file: byte fidelity cannot
    /// be promised back to disk over a lossy read.
    pub lossy: bool,
    /// Offsets in the normalized text of each newline that was CRLF in
    /// the file bytes. The splice ledger restores them on save, so the
    /// normalization the viewer needs never reaches the disk.
    pub crlf: Vec<u32>,
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
    // A book is a zip and full of NUL bytes; it routes before the sniff.
    if detect(path) == FileKind::Epub {
        let (mut document, toc, book) = super::epub::open_prefix(bytes)?;
        // Position memory falls back to the canonical path when the
        // metadata carries no identifier.
        if document.book_id.is_none() {
            document.book_id = path.canonicalize().ok().map(|p| p.display().to_string());
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
        });
    }
    if is_binary(&bytes) {
        anyhow::bail!("{} is not a text file", path.display());
    }
    let text = String::from_utf8_lossy(&bytes);
    let lossy = matches!(text, std::borrow::Cow::Owned(_));
    // Windows files carry CRLF; the plain-text path strips returns per
    // line, and everything downstream (offsets, rendering, copy as
    // markdown) assumes the source is clean of them. Each stripped
    // return leaves its normalized offset behind, so the splice ledger
    // can put every untouched ending back verbatim on save.
    let (text, crlf) = if text.contains("\r\n") {
        let mut out = String::with_capacity(text.len());
        let mut crlf = Vec::new();
        let mut rest = &*text;
        while let Some(i) = rest.find("\r\n") {
            out.push_str(&rest[..i]);
            crlf.push(out.len() as u32);
            out.push('\n');
            rest = &rest[i + 2..];
        }
        out.push_str(rest);
        (std::borrow::Cow::Owned(out), crlf)
    } else {
        (text, Vec::new())
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
        FileKind::Epub => unreachable!("books returned before the sniff"),
        FileKind::Undisplayable => unreachable!("refused before the sniff"),
        FileKind::Unknown => code_document(None, &text),
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
            highlights,
        } = &block.kind
        else {
            continue;
        };
        if highlights.len() < lines.len() {
            pending.push(PendingBlock {
                block: index,
                language: language.clone(),
                source: Arc::clone(&doc.source),
                lines: lines.clone(),
            });
        }
    }
    pending
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
        } = &mut block.kind
        else {
            continue;
        };
        *highlights = highlight::spans_until(&source, lines, language.as_deref(), deadline);
        if highlights.len() < lines.len() {
            pending.push(PendingBlock {
                block: index,
                language: language.clone(),
                source: Arc::clone(&source),
                lines: lines.clone(),
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
        lines, highlights, ..
    } = &mut block.kind
    else {
        return;
    };
    let end = (arrival.start_line + arrival.spans.len()).min(lines.len());
    if highlights.len() < end {
        highlights.resize(end, Vec::new());
    }
    for (offset, spans) in arrival.spans.iter().enumerate() {
        let line = arrival.start_line + offset;
        if line < end {
            highlights[line] = spans.clone();
        }
    }
}

/// A short notice (an open error) rendered as a plain document.
pub fn message(text: &str) -> Document {
    plain_document(text)
}

/// Every extension Oryx renders intentionally, for dialog filters.
pub fn recognized_extensions() -> Vec<&'static str> {
    ["md", "markdown", "txt", "epub"]
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
/// unstyled. The lines are ranges into the source: a code file was two
/// full copies of itself before layout ran, now it is one.
pub(crate) fn code_document(token: Option<&str>, text: &str) -> Document {
    let mut lines = source_lines(text);
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut block = Block::plain(BlockKind::CodeBlock {
        language: token.map(str::to_string),
        lines: CodeBody::verbatim(lines),
        highlights: Vec::new(),
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
/// Extensionless file names with a known language, matched exactly
/// against the file name. `detect` consults it before anything else, so
/// a `Dockerfile` or a `Makefile` opens colored instead of falling
/// through to the content sniff. A suffixed variant (`Dockerfile.dev`)
/// stays unknown and still opens as plain code through the sniff.
static WELL_KNOWN_NAMES: &[(&str, &str)] = &[
    ("Containerfile", "dockerfile"),
    ("Dockerfile", "dockerfile"),
    ("GNUmakefile", "makefile"),
    ("Makefile", "makefile"),
    ("makefile", "makefile"),
];

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

#[cfg(test)]
mod tests {
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
            },
        );
        let BlockKind::CodeBlock { highlights, .. } = &opened.document.blocks[0].kind else {
            panic!()
        };
        assert_eq!(highlights, &eager);
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
            lines, highlights, ..
        } = &mut doc.blocks[0].kind
        else {
            panic!("expected a code block")
        };
        *highlights = highlight::spans(&source, lines, Some("rust"));
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
}

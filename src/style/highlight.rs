//! Maps syntect parse scopes onto theme syntax roles at load time.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

/// Styled ranges for one code line.
pub type LineSpans = Vec<(Range<usize>, SyntaxRole)>;

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
}

/// Per-line styled ranges for a code block. Lines with no recognized
/// language come back as single Plain ranges.
pub fn spans(lines: &[String], language: Option<&str>) -> Vec<LineSpans> {
    spans_until(lines, language, None)
}

/// `spans` cut off at a deadline: only whole lines computed before the
/// deadline are returned, so the result is a prefix of the full output.
/// None means no deadline. The one-time grammar load happens before the
/// first deadline check and counts against the caller's budget.
pub fn spans_until(
    lines: &[String],
    language: Option<&str>,
    deadline: Option<Instant>,
) -> Vec<LineSpans> {
    let mut parser = Parser::new(language);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        out.push(parser.line(line));
    }
    out
}

/// Highlights a whole block in fixed-size chunks, handing each chunk and
/// its starting line to `deliver`. Delivery order is front to back;
/// `deliver` returning false stops between chunks. Returns whether the
/// block completed.
pub fn spans_chunked(
    lines: &[String],
    language: Option<&str>,
    chunk_size: usize,
    mut deliver: impl FnMut(usize, Vec<LineSpans>) -> bool,
) -> bool {
    let chunk_size = chunk_size.max(1);
    let mut parser = Parser::new(language);
    let mut start = 0;
    while start < lines.len() {
        let end = (start + chunk_size).min(lines.len());
        let chunk = lines[start..end].iter().map(|l| parser.line(l)).collect();
        if !deliver(start, chunk) {
            return false;
        }
        start = end;
    }
    true
}

/// Lines per background delivery. Small enough that the top of a huge
/// block colors quickly, large enough that fold-ins stay rare.
pub const CHUNK_LINES: usize = 512;

/// A code block whose highlights were not finished inside the open
/// budget; the lines are owned so the worker needs no document access.
pub struct PendingBlock {
    pub block: usize,
    pub language: Option<String>,
    pub lines: Vec<String>,
}

/// One chunk of computed highlights for a block, `spans[i]` covering
/// line `start_line + i`.
pub struct Arrival {
    pub block: usize,
    pub start_line: usize,
    pub spans: Vec<LineSpans>,
}

/// Owns the background highlight worker and its arrivals queue. One
/// generation is live at a time; starting again cancels the old worker
/// between chunks and its arrivals are dropped at drain.
pub struct Highlighter {
    arrivals: Arc<Mutex<Vec<(u64, Arrival)>>>,
    generation: Arc<AtomicU64>,
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
        }
    }

    /// Cancels any running worker, then highlights `pending` front to
    /// back on a fresh thread; each chunk lands in the arrivals queue
    /// and `waker` runs after it so the event loop can drain.
    pub fn start(&mut self, pending: Vec<PendingBlock>, waker: impl Fn() + Send + 'static) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if pending.is_empty() {
            return;
        }
        let arrivals = Arc::clone(&self.arrivals);
        let current = Arc::clone(&self.generation);
        std::thread::spawn(move || {
            for p in pending {
                let done = spans_chunked(
                    &p.lines,
                    p.language.as_deref(),
                    CHUNK_LINES,
                    |start, chunk| {
                        if current.load(Ordering::SeqCst) != generation {
                            return false;
                        }
                        let arrival = Arrival {
                            block: p.block,
                            start_line: start,
                            spans: chunk,
                        };
                        arrivals
                            .lock()
                            .expect("arrivals lock")
                            .push((generation, arrival));
                        waker();
                        true
                    },
                );
                if !done {
                    return;
                }
            }
        });
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
}

impl Parser {
    fn new(language: Option<&str>) -> Parser {
        let syntax = language
            .and_then(resolve_syntax)
            .unwrap_or_else(|| syntax_set().find_syntax_plain_text());
        Parser {
            parse: ParseState::new(syntax),
            stack: ScopeStack::new(),
        }
    }

    fn line(&mut self, line: &str) -> LineSpans {
        let text = format!("{line}\n");
        let ops = self
            .parse
            .parse_line(&text, syntax_set())
            .unwrap_or_default();
        let mut ranges: LineSpans = Vec::new();
        let mut last = 0usize;
        let mut push = |from: usize, to: usize, stack: &ScopeStack| {
            let to = to.min(line.len());
            if from < to {
                let role = role_for(stack);
                match ranges.last_mut() {
                    Some((prev, r)) if *r == role && prev.end == from => prev.end = to,
                    _ => ranges.push((from..to, role)),
                }
            }
        };
        for (index, op) in &ops {
            push(last, *index, &self.stack);
            last = (*index).max(last);
            let _ = self.stack.apply(op);
        }
        push(last, line.len(), &self.stack);
        ranges
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Tokens the bundled set does not answer directly, for two reasons.
///
/// A lookup matches a syntax name or one of its extensions, so a syntax
/// whose name carries punctuation is reachable only through an extension:
/// that is `batch` and `graphviz`, which are named "Batch File" and
/// "Graphviz (DOT)".
///
/// The rest have no grammar at all and take the closest one that does. The
/// substitute is chosen per language against real files, and the rule is
/// that under-coloring beats mis-coloring: a grammar that leaves a
/// construct plain is preferred to one that paints ordinary identifiers as
/// types. TOML takes JSON, whose typed literals match its values, while
/// INI takes Java Properties, whose bare `key=value` lines match its own;
/// the two formats look alike but their values do not. Kotlin and Swift
/// take Java, which covers their types, calls, strings and comments
/// without mis-reading anything, at the cost of leaving `fun`, `val`,
/// `func` and `let` uncolored.
const ALIASES: &[(&str, &str)] = &[
    ("batch", "bat"),
    ("csharp", "cs"),
    ("graphviz", "dot"),
    ("ini", "properties"),
    ("kotlin", "java"),
    ("swift", "java"),
    ("toml", "json"),
    ("tsx", "js"),
    ("typescript", "js"),
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

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn role_at(line: &[(Range<usize>, SyntaxRole)], pos: usize) -> SyntaxRole {
        line.iter()
            .find(|(r, _)| r.contains(&pos))
            .map(|(_, role)| *role)
            .unwrap_or(SyntaxRole::Plain)
    }

    #[test]
    fn rust_keyword_string_comment() {
        let src = lines(&["fn main() {", "    // a note", "    let s = \"hi\";", "}"]);
        let h = spans(&src, Some("rust"));
        assert_eq!(h.len(), 4);
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "fn");
        assert_eq!(role_at(&h[1], 6), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[2], 13), SyntaxRole::String, "string literal");
    }

    #[test]
    fn python_keyword() {
        let src = lines(&["def greet(name):", "    return name"]);
        let h = spans(&src, Some("python"));
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "def");
        assert_eq!(role_at(&h[1], 4), SyntaxRole::Keyword, "return");
    }

    #[test]
    fn tokens_reach_their_grammar_directly_or_by_alias() {
        assert_eq!(grammar_of("rust"), Some("Rust"));
        assert_eq!(grammar_of("haskell"), Some("Haskell"));
        assert_eq!(grammar_of("batch"), Some("Batch File"));
        assert_eq!(grammar_of("graphviz"), Some("Graphviz (DOT)"));
        assert_eq!(grammar_of("csharp"), Some("C#"));
        assert_eq!(grammar_of("typescript"), Some("JavaScript"));
        assert_eq!(grammar_of("toml"), Some("JSON"));
        assert_eq!(grammar_of("ini"), Some("Java Properties"));
        assert_eq!(grammar_of("kotlin"), Some("Java"));
        assert_eq!(grammar_of("swift"), Some("Java"));
    }

    #[test]
    fn a_language_with_no_grammar_and_no_alias_resolves_to_nothing() {
        assert_eq!(grammar_of("nosuchlang"), None);
    }

    #[test]
    fn toml_values_take_their_role_from_the_json_grammar() {
        let src = lines(&["version = \"0.7.0\"", "lto = true", "opt-level = 3"]);
        let h = spans(&src, Some("toml"));
        assert_eq!(role_at(&h[0], 12), SyntaxRole::String, "quoted value");
        assert_eq!(role_at(&h[1], 6), SyntaxRole::Keyword, "boolean");
        assert_eq!(role_at(&h[2], 12), SyntaxRole::Number, "number");
    }

    #[test]
    fn ini_keys_and_hash_comments_take_their_role_from_properties() {
        let src = lines(&["# a note", "Fullscreen=true"]);
        let h = spans(&src, Some("ini"));
        assert_eq!(role_at(&h[0], 3), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[1], 0), SyntaxRole::Keyword, "key");
    }

    #[test]
    fn unknown_language_is_plain() {
        let src = lines(&["anything at all"]);
        let h = spans(&src, Some("nosuchlang"));
        assert_eq!(h.len(), 1);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn no_language_is_plain() {
        let src = lines(&["plain text"]);
        let h = spans(&src, None);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn spans_until_past_deadline_computes_nothing() {
        let src = lines(&["fn main() {", "}"]);
        let h = spans_until(&src, Some("rust"), Some(Instant::now()));
        assert!(h.is_empty());
    }

    #[test]
    fn spans_until_without_deadline_matches_spans() {
        let src = lines(&["fn main() {", "    let s = \"hi\";", "}"]);
        assert_eq!(
            spans_until(&src, Some("rust"), None),
            spans(&src, Some("rust"))
        );
    }

    #[test]
    fn chunked_spans_concatenate_to_one_shot() {
        let src: Vec<String> = (0..10).map(|i| format!("let x{i} = {i}; // n")).collect();
        let mut starts = Vec::new();
        let mut joined = Vec::new();
        let complete = spans_chunked(&src, Some("rust"), 4, |start, chunk| {
            starts.push(start);
            joined.extend(chunk);
            true
        });
        assert!(complete);
        assert_eq!(starts, vec![0, 4, 8]);
        assert_eq!(joined, spans(&src, Some("rust")));
    }

    #[test]
    fn chunked_spans_stop_between_chunks_when_told() {
        let src: Vec<String> = (0..10).map(|i| format!("let x{i} = {i};")).collect();
        let mut deliveries = 0;
        let complete = spans_chunked(&src, Some("rust"), 4, |_, _| {
            deliveries += 1;
            false
        });
        assert!(!complete);
        assert_eq!(deliveries, 1);
    }

    #[test]
    fn worker_delivers_every_line_in_order() {
        let src: Vec<String> = (0..1200).map(|i| format!("let v{i} = {i};")).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.start(
            vec![PendingBlock {
                block: 3,
                language: Some("rust".into()),
                lines: src.clone(),
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
        assert_eq!(joined, spans(&src, Some("rust")));
    }

    #[test]
    fn restart_discards_stale_arrivals() {
        let src: Vec<String> = (0..600).map(|i| format!("let v{i} = {i};")).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut h = Highlighter::new();
        h.start(
            vec![PendingBlock {
                block: 0,
                language: None,
                lines: src,
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

    #[test]
    fn empty_lines_produce_empty_ranges() {
        let src = lines(&["", "x"]);
        let h = spans(&src, Some("rust"));
        assert_eq!(h.len(), 2);
        assert!(h[0].is_empty());
    }
}

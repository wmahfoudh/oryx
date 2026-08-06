//! Streaming parse: the prefix cut, the background parse worker, and the
//! swap that lands the worker's document. The design's Streaming parse
//! section is the contract.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::doc::model::{Block, BlockKind, DetailsGroup};

/// Prefix size the first frame parses; files at or under it take the
/// sync path.
pub const PREFIX_TARGET: usize = 128 * 1024;

/// How far the cut scan looks before giving up. A source with no blank
/// line outside a fence inside the bound parses synchronously.
pub const SCAN_BOUND: usize = 1024 * 1024;

/// Where the prefix ends: one byte past the first blank line at or past
/// the target that lies outside a fenced code block. None means the
/// whole source parses synchronously.
pub fn cut(source: &str) -> Option<usize> {
    cut_at(source, PREFIX_TARGET, SCAN_BOUND)
}

fn cut_at(source: &str, target: usize, bound: usize) -> Option<usize> {
    if source.len() <= target {
        return None;
    }
    let mut fence: Option<(char, usize)> = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if offset > bound {
            return None;
        }
        let text = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = text.trim_start();
        match (fence, fence_marker(trimmed)) {
            (None, Some((ch, run, _))) => fence = Some((ch, run)),
            (Some((ch, run)), Some((mch, mrun, pure))) if mch == ch && mrun >= run && pure => {
                fence = None;
            }
            _ => {}
        }
        if fence.is_none() && offset >= target && text.trim().is_empty() {
            return Some(offset + line.len());
        }
        offset += line.len();
    }
    None
}

/// A code fence marker at the start of a trimmed line: the fence
/// character, its run length, and whether nothing but whitespace follows,
/// which is what a closing fence requires. The scan is a heuristic; the
/// swap comparison is the correctness backstop, so a miss costs one
/// relayout, never wrong content.
fn fence_marker(trimmed: &str) -> Option<(char, usize, bool)> {
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = trimmed.chars().take_while(|c| *c == ch).count();
    if run < 3 {
        return None;
    }
    let pure = trimmed[run..].trim().is_empty();
    Some((ch, run, pure))
}

/// How the worker's full parse lands against the kept prefix.
#[derive(Debug)]
pub enum Swap {
    /// The prefix matched the full parse's head: append these blocks
    /// behind the kept ones, whose layout and highlights stay valid.
    Splice(Vec<Block>),
    /// The prefix disagreed: the full parse replaces the whole model and
    /// layout restarts.
    Replace(Vec<Block>),
}

/// Compares the prefix against the full parse's head and decides how the
/// delivery lands.
pub fn swap(prefix: &[Block], mut full: Vec<Block>) -> Swap {
    let matches = full.len() >= prefix.len()
        && prefix
            .iter()
            .zip(full.iter())
            .all(|(kept, incoming)| block_matches(kept, incoming));
    if matches {
        Swap::Splice(full.split_off(prefix.len()))
    } else {
        Swap::Replace(full)
    }
}

/// Whether the worker's block agrees with the kept one, highlights
/// aside: the prefix may already carry colors the worker never computed,
/// and colors are not content.
fn block_matches(kept: &Block, incoming: &Block) -> bool {
    if kept.quote_depth != incoming.quote_depth
        || kept.alert != incoming.alert
        || kept.range != incoming.range
        || kept.centered != incoming.centered
        || kept.details != incoming.details
    {
        return false;
    }
    match (&kept.kind, &incoming.kind) {
        (
            BlockKind::CodeBlock {
                language: kept_language,
                lines: kept_lines,
                ..
            },
            BlockKind::CodeBlock {
                language, lines, ..
            },
        ) => kept_language == language && kept_lines == lines,
        (kept, incoming) => kept == incoming,
    }
}

/// Adopts the full parse's details groups after a splice, carrying the
/// fold toggles the reader already made on the prefix's groups. Ids
/// align because the prefix matched block for block.
pub fn adopt_details(current: &[DetailsGroup], mut full: Vec<DetailsGroup>) -> Vec<DetailsGroup> {
    for (kept, incoming) in current.iter().zip(full.iter_mut()) {
        incoming.open = kept.open;
    }
    full
}

/// What a worker hands back: the full model, and for a book the grown
/// source the blocks index, which the prefix's source is a bit-for-bit
/// head of. A markdown delivery never changes the source.
#[derive(Debug)]
pub struct Delivered {
    pub blocks: Vec<Block>,
    pub details: Vec<DetailsGroup>,
    pub source: Option<std::sync::Arc<str>>,
}

/// A parked delivery and the generation that produced it.
type Delivery = (u64, Delivered);

/// Owns the background full parse. One generation is live at a time:
/// starting again or cancelling bumps it, the running worker bails at its
/// next check, and a stale delivery is dropped at drain.
pub struct ParseWorker {
    generation: Arc<AtomicU64>,
    slot: Arc<Mutex<Option<Delivery>>>,
    handle: Option<JoinHandle<()>>,
}

impl Default for ParseWorker {
    fn default() -> Self {
        ParseWorker {
            generation: Arc::new(AtomicU64::new(0)),
            slot: Arc::new(Mutex::new(None)),
            handle: None,
        }
    }
}

impl ParseWorker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses the whole source off the main thread and runs the waker
    /// when the blocks are ready. The worker shares the document's
    /// source through the `Arc`, owning no copy. Starting again cancels
    /// the running worker.
    pub fn start(&mut self, source: impl Into<Arc<str>>, waker: impl Fn() + Send + 'static) {
        let source: Arc<str> = source.into();
        self.start_with(
            move |bail| {
                crate::doc::markdown::parse_unless(source, bail).map(|document| Delivered {
                    blocks: document.blocks,
                    details: document.details,
                    source: None,
                })
            },
            waker,
        );
    }

    /// Runs any producing job off the main thread under the same
    /// generation, slot, and waker discipline; the book walk rides this.
    /// The job's bail hook answers true once the generation moves on.
    pub fn start_with(
        &mut self,
        job: impl FnOnce(&dyn Fn() -> bool) -> Option<Delivered> + Send + 'static,
        waker: impl Fn() + Send + 'static,
    ) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let slot = Arc::clone(&self.slot);
        let current = Arc::clone(&self.generation);
        self.handle = Some(std::thread::spawn(move || {
            let bail = move || current.load(Ordering::SeqCst) != generation;
            let Some(delivered) = job(&bail) else {
                return;
            };
            {
                let mut slot = slot.lock().expect("parse slot");
                if bail() {
                    return;
                }
                *slot = Some((generation, delivered));
            }
            waker();
        }));
    }

    /// Cancels the running worker without starting another; its delivery
    /// goes stale and any parked one is dropped.
    pub fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.slot.lock().expect("parse slot") = None;
        self.handle = None;
    }

    /// The current generation's delivery, once, when it has arrived.
    pub fn drain(&mut self) -> Option<Delivered> {
        let mut slot = self.slot.lock().expect("parse slot");
        let current = self.generation.load(Ordering::SeqCst);
        match slot.take() {
            Some((generation, delivered)) if generation == current => Some(delivered),
            _ => None,
        }
    }

    /// Blocks until the running worker finishes and hands over its
    /// delivery; None when nothing current is owed.
    pub fn finish(&mut self) -> Option<Delivered> {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    fn drain_within(worker: &mut ParseWorker, ms: u64) -> Option<Delivered> {
        for _ in 0..ms {
            if let Some(delivery) = worker.drain() {
                return Some(delivery);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    #[test]
    fn the_worker_delivers_the_full_parse() {
        let source = "# Title\n\npara one\n\n- item\n".to_string();
        let mut worker = ParseWorker::new();
        let woke = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&woke);
        worker.start(source.clone(), move || flag.store(true, Ordering::SeqCst));
        let delivered = drain_within(&mut worker, 2000).expect("a delivery arrives");
        assert_eq!(
            delivered.blocks,
            crate::doc::markdown::parse(source.as_str()).blocks
        );
        assert!(
            delivered.source.is_none(),
            "markdown never swaps the source"
        );
        assert!(woke.load(Ordering::SeqCst), "the waker ran");
        assert!(worker.drain().is_none(), "a delivery drains once");
    }

    #[test]
    fn a_restart_makes_the_first_delivery_stale() {
        let first = "# First\n\nfrom the first source\n".to_string();
        let second = "# Second\n\nfrom the second source\n".to_string();
        let mut worker = ParseWorker::new();
        worker.start(first, || {});
        worker.start(second.clone(), || {});
        let delivered = drain_within(&mut worker, 2000).expect("the live delivery arrives");
        assert_eq!(
            delivered.blocks,
            crate::doc::markdown::parse(second.as_str()).blocks
        );
    }

    #[test]
    fn a_spliced_delivery_carries_prefix_toggles() {
        let current = vec![
            DetailsGroup {
                parent: None,
                open: true,
            },
            DetailsGroup {
                parent: None,
                open: false,
            },
        ];
        let full = vec![
            DetailsGroup {
                parent: None,
                open: false,
            },
            DetailsGroup {
                parent: None,
                open: false,
            },
            DetailsGroup {
                parent: Some(1),
                open: true,
            },
        ];
        let adopted = adopt_details(&current, full);
        assert!(adopted[0].open, "the reader's toggle survives the splice");
        assert!(!adopted[1].open);
        assert!(adopted[2].open, "a tail group keeps its parsed state");
    }

    #[test]
    fn a_job_delivery_can_swap_the_source() {
        let mut worker = ParseWorker::new();
        worker.start_with(
            |_| {
                Some(Delivered {
                    blocks: Vec::new(),
                    details: Vec::new(),
                    source: Some(Arc::from("the grown book source")),
                })
            },
            || {},
        );
        let delivered = worker.finish().expect("the job delivers");
        assert_eq!(delivered.source.as_deref(), Some("the grown book source"));
    }

    #[test]
    fn a_cancelled_worker_delivers_nothing() {
        let source = "# Title\n\nbody\n".to_string();
        let mut worker = ParseWorker::new();
        worker.start(source, || {});
        worker.cancel();
        assert!(drain_within(&mut worker, 100).is_none());
    }

    #[test]
    fn a_matching_prefix_splices_and_keeps_its_highlights() {
        let source = "```rust\nfn a() {}\n```\n\npara one\n\npara two\n";
        let cut = cut_at(source, 1, 1024).expect("the fixture cuts");
        let mut doc = crate::doc::markdown::parse(&source[..cut]);
        doc.source = std::sync::Arc::from(source);
        let BlockKind::CodeBlock {
            lines, highlights, ..
        } = &mut doc.blocks[0].kind
        else {
            panic!("the prefix starts with the code block")
        };
        *highlights = crate::style::highlight::spans(source, lines, Some("rust"));
        let full = crate::doc::markdown::parse(source);
        let Swap::Splice(tail) = swap(&doc.blocks, full.blocks) else {
            panic!("a clean cut splices")
        };
        doc.blocks.extend(tail);
        let full = crate::doc::markdown::parse(source);
        assert_eq!(doc.blocks.len(), full.blocks.len());
        assert_eq!(
            doc.blocks[1..],
            full.blocks[1..],
            "the tail is the full parse's"
        );
        let BlockKind::CodeBlock { highlights, .. } = &doc.blocks[0].kind else {
            panic!("the code block survived")
        };
        assert!(!highlights.is_empty(), "computed colors survive the swap");
    }

    #[test]
    fn a_reference_link_below_the_cut_replaces() {
        let source = "a [site][ref] paragraph\n\ntail paragraph\n\n[ref]: https://example.com\n";
        let cut = cut_at(source, 4, 1024).expect("the fixture cuts");
        let mut doc = crate::doc::markdown::parse(&source[..cut]);
        doc.source = std::sync::Arc::from(source);
        let full = crate::doc::markdown::parse(source);
        let Swap::Replace(blocks) = swap(&doc.blocks, full.blocks) else {
            panic!("an unresolved reference cannot splice")
        };
        assert_eq!(blocks, crate::doc::markdown::parse(source).blocks);
    }

    #[test]
    fn a_full_parse_shorter_than_the_prefix_replaces() {
        let prefix = crate::doc::markdown::parse("one\n\ntwo\n\nthree\n");
        let full = crate::doc::markdown::parse("one\n");
        assert!(matches!(
            swap(&prefix.blocks, full.blocks),
            Swap::Replace(_)
        ));
    }

    #[test]
    fn finish_joins_the_worker() {
        let source = "# Title\n\npara\n\n> quote\n".to_string();
        let mut worker = ParseWorker::new();
        assert!(worker.finish().is_none(), "nothing owed before a start");
        worker.start(source.clone(), || {});
        let delivered = worker.finish().expect("finish waits for the blocks");
        assert_eq!(
            delivered.blocks,
            crate::doc::markdown::parse(source.as_str()).blocks
        );
        assert!(worker.finish().is_none(), "a delivery is owed once");
    }

    #[test]
    fn the_cut_lands_after_the_first_blank_line_past_the_target() {
        let source = "alpha\n\nbeta\n\ngamma\n\ndelta\n";
        // Blank lines start at 6, 12 and 19; the first at or past the
        // target of 8 starts at 12, so the prefix ends at 13.
        assert_eq!(cut_at(source, 8, 1024), Some(13));
        assert!(source[13..].starts_with("gamma"));
    }

    #[test]
    fn a_whitespace_only_line_is_blank() {
        let source = "alpha\n  \t\nbeta\n";
        assert_eq!(cut_at(source, 2, 1024), Some(10));
    }

    #[test]
    fn a_fence_straddling_the_target_pushes_the_cut_past_its_close() {
        let source = "intro\n\n```\ncode\n\nmore\n```\n\nafter\n";
        // The blank line inside the fence starts at 16; the one after the
        // close starts at 26. A target inside the fence must skip to it.
        assert_eq!(cut_at(source, 12, 1024), Some(27));
        assert!(source[27..].starts_with("after"));
    }

    #[test]
    fn a_longer_fence_swallows_a_shorter_marker() {
        let source = "````\n```\n\n````\n\ntail\n";
        // The ``` line and the blank after it sit inside the ````
        // fence; the first blank outside starts at 15.
        assert_eq!(cut_at(source, 1, 1024), Some(16));
        assert!(source[16..].starts_with("tail"));
    }

    #[test]
    fn tilde_fences_track_like_backtick_fences() {
        let source = "~~~\ntext\n\n~~~\n\ntail\n";
        assert_eq!(cut_at(source, 1, 1024), Some(15));
    }

    #[test]
    fn sources_at_or_under_the_target_have_no_cut() {
        let source = "alpha\n\nbeta\n";
        assert_eq!(cut_at(source, source.len(), 1024), None);
        assert_eq!(cut_at("", 0, 1024), None);
    }

    #[test]
    fn a_blank_line_past_the_bound_is_not_found() {
        let source = format!("{}\n\ntail\n", "one line without a break ".repeat(8));
        assert_eq!(cut_at(&source, 8, 64), None);
    }

    #[test]
    fn a_source_with_no_blank_line_has_no_cut() {
        let source = "first line\nsecond line\nthird line\n";
        assert_eq!(cut_at(source, 4, 1024), None);
    }

    #[test]
    fn the_public_cut_uses_the_real_target() {
        let para = "a paragraph of filler text\n\n";
        let source = para.repeat(2 + PREFIX_TARGET / para.len());
        let cut = cut(&source).expect("a broken source past the target cuts");
        assert!(cut > PREFIX_TARGET);
        assert!(cut <= PREFIX_TARGET + 2 * para.len());
        assert!(source[..cut].ends_with("\n\n"));
    }
}

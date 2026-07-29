//! The shaping pool: workers shape steps ahead of the assembler, one
//! font system each, cloned from a seed so no worker pays a system font
//! scan. The assembler stays the only writer of the layout; workers only
//! fill per-step scratches, and a generation bump orphans everything in
//! flight.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::doc::model::Block;
use crate::layout::{LayoutDoc, ViewConfig};
use crate::style::fonts::FontSeed;
use crate::style::highlight::SyntaxRole;
use crate::style::theme::Theme;

/// One step's identity inside a pass: the order position, and the line
/// inside an open code block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StepKey {
    pub position: usize,
    pub line: usize,
}

/// What a worker shapes against for one pass generation. The source
/// rides along so a cloned block's verbatim spans resolve their text.
pub struct ShapeCtx {
    pub theme: Theme,
    pub cfg: ViewConfig,
    pub source: Arc<str>,
}

/// One claimed unit of shaping: a whole block's kind emission, or one
/// code line.
pub(crate) enum Work {
    Block {
        block: Block,
        block_index: usize,
        heading: Option<u8>,
        base_size: f32,
        x_base: f32,
        avail: f32,
    },
    CodeLine {
        line: String,
        segments: Vec<(Range<usize>, SyntaxRole)>,
        block_index: usize,
        line_index: usize,
        x0: f32,
        size: f32,
        line_height: f32,
        wrap_width: f32,
    },
}

pub(crate) struct Job {
    pub generation: u64,
    pub key: StepKey,
    pub ctx: Arc<ShapeCtx>,
    pub work: Work,
}

/// A shaped step parked for the assembler.
pub(crate) struct Shaped {
    pub scratch: LayoutDoc,
    pub height: f32,
}

struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    ready: Mutex<HashMap<(u64, StepKey), Shaped>>,
    wake: Condvar,
    generation: AtomicU64,
    shutdown: AtomicBool,
    completed: AtomicUsize,
}

pub struct ShapePool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl ShapePool {
    /// Spawns `count` workers, each with its own store cloned from the
    /// seed and a media cache nothing ever fills, since image-bearing
    /// blocks never enter the pool.
    pub fn new(count: usize, seed: &FontSeed) -> ShapePool {
        let shared = Arc::new(Shared {
            jobs: Mutex::new(VecDeque::new()),
            ready: Mutex::new(HashMap::new()),
            wake: Condvar::new(),
            generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            completed: AtomicUsize::new(0),
        });
        let workers = (0..count.max(1))
            .map(|_| {
                let shared = Arc::clone(&shared);
                let store = crate::style::fonts::FontStore::pooled(seed);
                std::thread::spawn(move || worker(shared, store))
            })
            .collect();
        ShapePool { shared, workers }
    }

    /// How many workers shape, for sizing the seeding window.
    pub fn width(&self) -> usize {
        self.workers.len()
    }

    /// Claims the pool for a new pass: everything in flight goes stale.
    /// Returns the new generation.
    pub fn begin(&self) -> u64 {
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.jobs.lock().expect("pool jobs").clear();
        self.shared.ready.lock().expect("pool ready").clear();
        generation
    }

    /// The live generation, for a pass to notice it was superseded.
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::SeqCst)
    }

    /// Steps shaped by workers over the pool's lifetime, for tests to
    /// prove the pool did real work.
    pub fn completed(&self) -> usize {
        self.shared.completed.load(Ordering::SeqCst)
    }

    /// Queues work for the workers.
    pub(crate) fn submit(&self, job: Job) {
        self.shared.jobs.lock().expect("pool jobs").push_back(job);
        self.shared.wake.notify_one();
    }

    /// Jobs claimed but not yet consumed, for the seeding window.
    pub(crate) fn backlog(&self) -> usize {
        self.shared.jobs.lock().expect("pool jobs").len()
            + self.shared.ready.lock().expect("pool ready").len()
    }

    /// The shaped step for `key`, if a worker got there first. Entries
    /// behind `key` are orphans the assembler shaped itself; they drop
    /// here so the window never silts up.
    pub(crate) fn take(&self, generation: u64, key: StepKey) -> Option<Shaped> {
        let mut ready = self.shared.ready.lock().expect("pool ready");
        ready.retain(|(g, k), _| *g == generation && *k >= key);
        ready.remove(&(generation, key))
    }
}

impl Drop for ShapePool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker(shared: Arc<Shared>, mut fonts: crate::style::fonts::FontStore) {
    // Image-bearing blocks never enter the pool, so this cache is never
    // filled; it only satisfies the shaper's signature.
    let mut media = crate::doc::images::MediaCache::new(std::env::temp_dir());
    loop {
        let job = {
            let mut jobs = shared.jobs.lock().expect("pool jobs");
            loop {
                if shared.shutdown.load(Ordering::SeqCst) {
                    return;
                }
                match jobs.pop_front() {
                    Some(job) => break job,
                    None => jobs = shared.wake.wait(jobs).expect("pool wait"),
                }
            }
        };
        if job.generation != shared.generation.load(Ordering::SeqCst) {
            continue;
        }
        let mut scratch = LayoutDoc::default();
        let height = match &job.work {
            Work::Block {
                block,
                block_index,
                heading,
                base_size,
                x_base,
                avail,
            } => super::engine::shape_kind(
                &mut fonts,
                &job.ctx.theme,
                &job.ctx.cfg,
                &job.ctx.source,
                &mut media,
                block,
                *block_index,
                *heading,
                *base_size,
                *x_base,
                *avail,
                &mut scratch,
            ),
            Work::CodeLine {
                line,
                segments,
                block_index,
                line_index,
                x0,
                size,
                line_height,
                wrap_width,
            } => super::engine::shape_code_line_step(
                &mut fonts,
                &job.ctx.theme,
                &job.ctx.cfg,
                line,
                segments,
                *block_index,
                *line_index,
                *x0,
                *size,
                *line_height,
                *wrap_width,
                &mut scratch,
            ),
        };
        if job.generation == shared.generation.load(Ordering::SeqCst) {
            shared
                .ready
                .lock()
                .expect("pool ready")
                .insert((job.generation, job.key), Shaped { scratch, height });
            shared.completed.fetch_add(1, Ordering::SeqCst);
        }
    }
}

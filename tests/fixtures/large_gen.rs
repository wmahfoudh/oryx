//! Generates large representative documents for the perf test. The
//! markdown fixture mixes headings, styled paragraphs, lists, code, a
//! table, and quotes; the code fixture is plain Rust source for the
//! whole-file code view. Both repeat a section until the requested size
//! is reached.

pub fn generate(bytes: usize) -> String {
    let section = "## Section heading\n\n\
        A paragraph with **bold**, *italic*, `inline code`, and a\n\
        [link](https://example.com) that wraps across lines when narrow.\n\n\
        - first item with some text\n\
        - second item with `code`\n\
          - nested item\n\n\
        ```rust\n\
        fn compute(n: u64) -> u64 {\n\
            // a comment long enough to be representative of real code\n\
            (0..n).map(|i| i * i).sum()\n\
        }\n\
        ```\n\n\
        |name|value|note|\n\
        |----|-----|----|\n\
        |alpha|1|first|\n\
        |beta|2|second|\n\n\
        > A quoted paragraph closing the section.\n\n";
    let mut out = String::with_capacity(bytes + section.len());
    out.push_str("# Performance fixture\n\n");
    while out.len() < bytes {
        out.push_str(section);
    }
    out
}

pub fn generate_code(bytes: usize) -> String {
    let section = r#"/// Aggregates samples into fixed-width buckets for the report.
pub struct Histogram {
    buckets: Vec<u64>,
    width: f64,
}

impl Histogram {
    pub fn new(width: f64, count: usize) -> Self {
        // Bucket zero swallows negatives so record never branches.
        Histogram { buckets: vec![0; count.max(1)], width }
    }

    pub fn record(&mut self, value: f64) {
        let index = (value / self.width).max(0.0) as usize;
        let last = self.buckets.len() - 1;
        self.buckets[index.min(last)] += 1;
    }

    pub fn summary(&self) -> String {
        let total: u64 = self.buckets.iter().sum();
        format!("{} samples in {} buckets", total, self.buckets.len())
    }
}

"#;
    let mut out = String::with_capacity(bytes + section.len());
    out.push_str("//! Generated fixture: representative Rust source.\n\n");
    while out.len() < bytes {
        out.push_str(section);
    }
    out
}

//! Generates a large representative markdown document for the perf test:
//! headings, styled paragraphs, lists, code, a table, and quotes repeat
//! until the requested size is reached.

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

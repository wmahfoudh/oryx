//! File loading and type detection by extension.

use std::path::Path;

use crate::doc::model::{Block, BlockKind, Document, Span};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FileKind {
    Markdown,
    /// Carries the syntax token used for highlighting (`"rust"`, `"python"`).
    Code(&'static str),
    Plain,
}

pub fn detect(path: &Path) -> FileKind {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return FileKind::Plain;
    };
    let ext = ext.to_ascii_lowercase();
    if ext == "md" || ext == "markdown" {
        return FileKind::Markdown;
    }
    match CODE_EXTENSIONS.binary_search_by_key(&ext.as_str(), |(k, _)| k) {
        Ok(i) => FileKind::Code(CODE_EXTENSIONS[i].1),
        Err(_) => FileKind::Plain,
    }
}

pub fn open(path: &Path) -> anyhow::Result<Document> {
    let bytes =
        std::fs::read(path).map_err(|e| anyhow::anyhow!("cannot open {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(match detect(path) {
        FileKind::Markdown => super::markdown::parse(&text),
        FileKind::Code(token) => code_document(token, &text),
        FileKind::Plain => plain_document(&text),
    })
}

/// A short notice (an open error) rendered as a plain document.
pub fn message(text: &str) -> Document {
    plain_document(text)
}

/// Every extension Oryx renders intentionally, for dialog filters.
pub fn recognized_extensions() -> Vec<&'static str> {
    ["md", "markdown", "txt"]
        .into_iter()
        .chain(CODE_EXTENSIONS.iter().map(|(ext, _)| *ext))
        .collect()
}

/// The whole file as a single highlighted code block.
fn code_document(token: &str, text: &str) -> Document {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let highlights = crate::style::highlight::spans(&lines, Some(token));
    let mut block = Block::plain(BlockKind::CodeBlock {
        language: Some(token.to_string()),
        lines,
        highlights,
    });
    block.range = 0..text.len();
    Document {
        blocks: vec![block],
        source: text.to_string(),
    }
}

/// Paragraphs split on blank lines; line breaks inside a paragraph are
/// preserved as newline spans so the lines sit flush in layout.
fn plain_document(text: &str) -> Document {
    let mut blocks = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut offset = 0;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            flush_plain(&mut blocks, &mut spans);
        } else {
            if !spans.is_empty() {
                spans.push(Span::plain("\n"));
            }
            let mut span = Span::plain(line);
            span.range = offset..offset + line.len();
            spans.push(span);
        }
        offset += raw.len() + 1;
    }
    flush_plain(&mut blocks, &mut spans);
    Document {
        blocks,
        source: text.to_string(),
    }
}

fn flush_plain(blocks: &mut Vec<Block>, spans: &mut Vec<Span>) {
    if spans.is_empty() {
        return;
    }
    let spans = std::mem::take(spans);
    let with_range: Vec<_> = spans.iter().filter(|s| !s.range.is_empty()).collect();
    let range = match (with_range.first(), with_range.last()) {
        (Some(first), Some(last)) => first.range.start..last.range.end,
        _ => 0..0,
    };
    let mut block = Block::plain(BlockKind::Paragraph { spans });
    block.range = range;
    blocks.push(block);
}

/// Extension to highlight token, sorted by extension for binary search.
static CODE_EXTENSIONS: &[(&str, &str)] = &[
    ("bash", "bash"),
    ("c", "c"),
    ("cc", "cpp"),
    ("cfg", "ini"),
    ("cpp", "cpp"),
    ("cs", "csharp"),
    ("css", "css"),
    ("go", "go"),
    ("h", "c"),
    ("hpp", "cpp"),
    ("html", "html"),
    ("ini", "ini"),
    ("java", "java"),
    ("js", "javascript"),
    ("json", "json"),
    ("jsx", "javascript"),
    ("kt", "kotlin"),
    ("lua", "lua"),
    ("mjs", "javascript"),
    ("php", "php"),
    ("pl", "perl"),
    ("py", "python"),
    ("rb", "ruby"),
    ("rs", "rust"),
    ("sh", "bash"),
    ("sql", "sql"),
    ("swift", "swift"),
    ("toml", "toml"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("xml", "xml"),
    ("yaml", "yaml"),
    ("yml", "yaml"),
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
        ] {
            assert_eq!(detect_ext(name), FileKind::Code(token), "{name}");
        }
    }

    #[test]
    fn unknown_and_missing_extensions_are_plain() {
        assert_eq!(detect_ext("notes.xyz"), FileKind::Plain);
        assert_eq!(detect_ext("README"), FileKind::Plain);
        assert_eq!(detect_ext("archive.txt"), FileKind::Plain);
    }

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("oryx-load-{}-{name}", std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn code_file_becomes_one_code_block() {
        let path = temp_file("t.py", "def f():\n    return 1\n");
        let d = open(&path).unwrap();
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
        assert_eq!(lines, &["def f():", "    return 1"]);
        assert_eq!(highlights.len(), 2);
    }

    #[test]
    fn plain_file_splits_paragraphs_on_blank_lines() {
        let path = temp_file("t.txt", "line one\nline two\n\nsecond para\n");
        let d = open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(d.blocks.len(), 2);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!()
        };
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "line one\nline two");
        assert!(matches!(&d.blocks[1].kind, BlockKind::Paragraph { .. }));
    }

    #[test]
    fn markdown_file_parses_as_markdown() {
        let path = temp_file("t.md", "# Title\n\nbody\n");
        let d = open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(&d.blocks[0].kind, BlockKind::Heading { .. }));
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(open(Path::new("/nonexistent/oryx-missing.md")).is_err());
    }

    #[test]
    fn message_becomes_a_plain_document() {
        let d = message("cannot open /x: denied");
        assert_eq!(d.blocks.len(), 1);
        let BlockKind::Paragraph { spans } = &d.blocks[0].kind else {
            panic!("expected a paragraph")
        };
        assert_eq!(spans[0].text, "cannot open /x: denied");
    }

    #[test]
    fn recognized_extensions_cover_the_renderable_set() {
        let exts = recognized_extensions();
        for e in ["md", "markdown", "txt", "rs", "py", "toml", "yaml"] {
            assert!(exts.contains(&e), "{e} missing");
        }
    }
}

<p align="center">
<img src="https://img.shields.io/badge/this_is-oryx-orange" height="20">
<img src="https://img.shields.io/badge/rendering-markdown-blue" height="20">
<img src="https://img.shields.io/badge/drawn_on_the-CPU-black" height="20">
<img src="https://img.shields.io/badge/startup-instant-brightgreen" height="20">
</p>

# This is Oryx, showing a markdown file

The document on your screen is a sample being rendered by Oryx itself:
**bold**, *italic*, ~~strikethrough~~, `inline code`, a
[link](https://codeberg.org/wmahfoudh/oryx), smart punctuation, and emoji
:sparkles: drawn natively, with no browser engine anywhere.

> [!TIP]
> Everything below is live markdown too: highlighted code, a real table,
> task lists, footnotes, and math literals.

## What code looks like

```rust
// Thirty languages highlighted, long lines wrapping inside the panel.
fn open(path: &Path) -> anyhow::Result<Document> {
    Ok(parse(&fs::read_to_string(path)?))
}
```

## What tables and lists look like

| Try | Shortcut |
|-----|----------|
| Thirty themes, browsed live | Ctrl+T |
| Find in document | Ctrl+F |
| Every other shortcut | F1 |

- [x] Render this sample
- [x] Stay one small binary
- [ ] Ever need a web engine[^1]

Math renders as styled literals, $\sum x_i^2$, and footnote references
click-jump to their definitions.

[^1]: Never. Oryx draws every pixel itself.

# Oryx

<img src="assets/icon/oryx.svg" width="96" alt="Oryx logo">

A fast markdown viewer. Instant startup, smooth scrolling, beautiful typography. No webview, no Electron, no GPU required.

Oryx renders on the CPU through a purpose-built layout engine, so it starts in milliseconds and scrolls at a constant cost on documents of any length.

## Status

Oryx is under active development. Rendering is in place for headings, text styles, syntax highlighted code, blockquotes, lists and task lists, tables, horizontal rules, and images including SVG. Interaction currently covers mouse and keyboard scrolling. Theme switching, font settings, selection and copy, a folder sidebar, and file association are planned and specified.

## Usage

```
oryx <file>
```

Opens markdown (`.md`, `.markdown`), source code files with syntax highlighting, or any text file.

| Key | Action |
|---|---|
| `Arrow Up` / `Arrow Down` | Scroll by line |
| `Page Up` / `Page Down`, `Space` / `Shift+Space` | Scroll by page |
| `Home` / `End` | Jump to top or bottom |
| `Escape` | Quit |

The scrollbar on the right edge can be dragged; the mouse wheel scrolls.

## Rendering

- Headings carry six independent theme colors, one per level.
- Bold, italic, and strikethrough each have their own color, not only a style.
- Code blocks are syntax highlighted panels; inline code sits in a pill.
- Tables size their columns to content: compact tables stay compact, wordy columns use the available width and wrap inside it.
- Images render inline, scaled down to the content width, never scaled up. SVG rasterizes at its intrinsic size. Broken paths show a bordered placeholder with the alt text.
- GitHub alerts, footnotes, math, emoji shortcodes, and YAML frontmatter are parsed and render progressively as features land.

## Themes

Two themes ship today: Oryx Light, warm paper with red, gold, and olive heading hues, and Oryx Dark, the same identity on warm dark ground. A theme is a single TOML file with 49 color roles covering every element independently; missing keys fall back to built-in defaults, and a malformed file is skipped, never fatal. Custom themes go in the `themes/` folder next to the binary.

## Building

Requires Rust 1.80 or later.

```
cargo build --release
```

The binary expects the `themes/` folder alongside it (or in the working directory during development). `make check` runs the full verification: formatting, clippy for Linux, Windows, and macOS targets, build, and tests.

## Fonts

DejaVu Sans and Courier Prime are embedded in the binary. DejaVu is distributed under the DejaVu Fonts License; Courier Prime under the SIL Open Font License.

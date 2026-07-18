# Oryx

<img src="assets/icon/oryx.svg" width="96" alt="Oryx logo">

A fast markdown viewer. Instant startup, smooth scrolling, beautiful typography. No webview, no Electron, no GPU required.

Oryx renders on the CPU through a purpose-built layout engine, so it starts in milliseconds and scrolls at a constant cost on documents of any length.

## Status

Oryx is under active development. Rendering is in place for headings, text styles, syntax highlighted code, blockquotes, lists and task lists, tables, horizontal rules, images including SVG, and clickable links. Interaction covers scrolling, link navigation, text selection with plain or markdown copy, a theme browser and theme editor, font and size settings, and zoom. A shortcuts help overlay, a folder sidebar, remote images, and file association are planned and specified.

## Usage

```
oryx [--theme <name>] <file>
```

Opens markdown (`.md`, `.markdown`), source code files with syntax highlighting, or any text file. `--theme` selects a theme from the `themes/` folder by file name, for example `--theme nord`.

| Key | Action |
|---|---|
| `Arrow Up` / `Arrow Down` | Scroll by line |
| `Page Up` / `Page Down`, `Space` / `Shift+Space` | Scroll by page |
| `Home` / `End` | Jump to top or bottom |
| `Ctrl+A` | Select the whole document |
| `Ctrl+C` | Copy the selection as plain text |
| `Ctrl+Shift+C` | Copy the selection as markdown |
| `Ctrl+T` | Theme browser |
| `Ctrl+,` | Settings: fonts and sizes |
| `Ctrl+Plus` / `Ctrl+Minus` / `Ctrl+0` | Zoom in, out, reset |
| `Escape` | Close the open panel, otherwise quit |

The scrollbar on the right edge can be dragged; the mouse wheel scrolls. Clicking a link opens it in the browser; anchor links jump inside the document. Dragging over text selects it.

The theme browser lists every theme with preview swatches; clicking or Enter applies one, and each row carries edit, duplicate, and delete actions, with double-click renaming. The editor covers all 49 color roles with a color picker and hex entry, restyling the document live; editing a shipped theme works on a copy. Settings choose the body and code fonts from the system list, previewed in their own faces, and step both sizes. Theme, fonts, and sizes persist across launches; zoom is per session.

## Rendering

- Headings carry six independent theme colors, one per level.
- Bold, italic, and strikethrough each have their own color, not only a style.
- Code blocks are syntax highlighted panels; inline code sits in a pill.
- Tables size their columns to content: compact tables stay compact, wordy columns use the available width and wrap inside it.
- Images render inline, scaled down to the content width, never scaled up. SVG rasterizes at its intrinsic size. Broken paths show a bordered placeholder with the alt text.
- GitHub alerts, footnotes, math, emoji shortcodes, and YAML frontmatter are parsed and render progressively as features land.

## Themes

Thirty themes ship with Oryx. A theme is a single TOML file with 49 color roles covering every element independently; missing keys fall back to built-in defaults, and a malformed file is skipped, never fatal. Custom themes go in the `themes/` folder next to the binary, and the built-in browser and editor manage them without leaving the app.

Eight are original designs: `oryx-light` and `oryx-dark` (the default warm identity), `oryx-sand` and `oryx-night` (desert day and night), `inkstone` (near-monochrome ink with a vermilion accent), `ember` (charcoal with a fire-gradient), `meadow` (dew-green with wildflower accents), and `slate` (disciplined cool gray).

The rest adapt permissively licensed editor palettes, all MIT, with thanks to their authors:

- Dracula ([draculatheme.com](https://draculatheme.com))
- Nord ([nordtheme.com](https://www.nordtheme.com))
- Gruvbox dark and light ([morhetz/gruvbox](https://github.com/morhetz/gruvbox))
- Catppuccin Mocha and Latte ([catppuccin.com](https://catppuccin.com))
- Tokyo Night ([enkia/tokyo-night-vscode-theme](https://github.com/enkia/tokyo-night-vscode-theme))
- Solarized dark and light by Ethan Schoonover ([ethanschoonover.com/solarized](https://ethanschoonover.com/solarized))
- One Dark ([atom](https://github.com/atom/atom))
- Everforest dark and light ([sainnhe/everforest](https://github.com/sainnhe/everforest))
- Rosé Pine and Rosé Pine Dawn ([rosepinetheme.com](https://rosepinetheme.com))
- Kanagawa ([rebelot/kanagawa.nvim](https://github.com/rebelot/kanagawa.nvim))
- Ayu Mirage and Light ([ayu-theme](https://github.com/ayu-theme/ayu-colors))
- Night Owl by Sarah Drasner ([sdras/night-owl-vscode-theme](https://github.com/sdras/night-owl-vscode-theme))
- Horizon ([jolaleye/horizon-theme-vscode](https://github.com/jolaleye/horizon-theme-vscode))
- Flexoki dark and light by Steph Ango ([stephango.com/flexoki](https://stephango.com/flexoki))
- GitHub Light ([primer/primitives](https://github.com/primer/primitives))

## Building

Requires Rust 1.80 or later.

```
cargo build --release
```

The binary expects the `themes/` folder alongside it (or in the working directory during development). `make check` runs the full verification: formatting, clippy for Linux, Windows, and macOS targets, build, and tests.

## Fonts

DejaVu Sans and Courier Prime are embedded in the binary. DejaVu is distributed under the DejaVu Fonts License; Courier Prime under the SIL Open Font License.

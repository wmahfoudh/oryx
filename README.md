<div align="center">

# Oryx

**A fast, native viewer for markdown and code.**

*Open it. Read it. Export it. Close it. All in seconds, in one small binary.*

![Platforms](https://img.shields.io/badge/Platforms-Linux%20%7C%20Windows%20%7C%20macOS-blue)
![License](https://img.shields.io/badge/License-GPL--3.0-orange)

[What it renders](#what-it-renders) •
[Tools](#tools) •
[Themes](#themes) •
[PDF export](#export-to-pdf) •
[Install](#install) •
[Performance](#performance)

<br>

![Oryx rendering a markdown document](screenshots/formatting.png)

</div>

## Why Oryx

Oryx is built mainly for the reading case: the file you want to open fast, enjoy reading, maybe export to PDF and close, without an editor or a browser tab in the way.

- **Instant**: A document is on screen in well under 100 ms from cold, even an 8 MB file.
- **Distraction-free**: No panes, no toolbars, no menus. `F1` lists the shortcuts, `Esc` closes whatever is open.
- **Beautiful**: 31 themes addressing 51 color roles, for reading and for PDF export alike.
- **Self-contained**: One binary and a folder of themes. No browser engine, no runtime, no GPU requirement.
- **Runs anywhere**: Performs the same on any desktop: a new laptop or an old machine with no graphics card.

## What it renders

### Markdown, the whole everyday set

Headings, bold, italic, strikethrough, inline code, links and bare URLs, nested blockquotes, horizontal rules, smart quotes and dashes, and emoji shortcodes like `:tada:`. Ordered, unordered and task lists nest as deep as needed, and a wrapped line aligns with the text above it, not with the bullet. Tables keep per-column alignment, shade alternating rows and wrap long cells, so a wide table never runs off the page.

### Code, highlighted

Fenced blocks get a bordered panel and syntax colors for the languages most people write, and a line too long for the panel wraps inside it. Oryx also opens source files directly and renders the whole file as one highlighted document. Close to a hundred extensions carry colors, from Rust and Python through Haskell, LaTeX, Makefiles and diffs. Any other text file opens in the code font, and a binary is announced in one line.

![Oryx rendering highlighted code](screenshots/code.png)

### GitHub flavor and more

All five GitHub alert kinds are styled, each with its own color and title. A YAML frontmatter header becomes a small metadata panel above the document. TeX math comes out with real symbols, inline or centered on a line of its own. Footnote markers sit raised in the text and link to their definitions, gathered at the foot of the document.

**Images and badges** render in place: PNG, JPEG, GIF, WebP or SVG. Remote images are fetched in the background and cached on disk, so a README covered in badges comes up immediately the second time it is opened, and keeps working offline. If a path is broken, it gets a placeholder carrying the alt text.

**Basic HTML** is handled too: centered blocks, images at a set width or height, rows of clickable badges, line breaks, and the inline tags down to `sub` and `sup`.

![Oryx rendering a GitHub style README](screenshots/github.png)

## Tools

- **Find in document**: `Ctrl+F` searches text. The search is smart about case: `oryx` matches Oryx, ORYX and oryx, while `Oryx` performs an exact match. A match can cross styling, so `fast viewer` is found even when it was written as **fast** *viewer*.
- **Select and copy**: `Ctrl+C` copies a selection as plain text. `Ctrl+Shift+C` copies the original markdown of the selection.
- **Sidebar**: A folder sidebar on `Ctrl+B` shows the tree around the open file and can be driven entirely from the keyboard.
- **Open file**: `Ctrl+O` opens the native file dialog.
- **Live reload**: `F5` reloads a file being edited elsewhere.
- **Zoom**: `Ctrl+Plus` (in) and `Ctrl+Minus` (out).
- **Persistence**: Window geometry, the active theme, the sidebar and the last folder are all saved and restored at every start.

## Themes

Thirty-one themes ship with Oryx. Each is a single TOML file with **51 color roles**, so every element can be colored on its own. A missing key falls back to a default; a malformed file is skipped, and the active theme stays.

Press `Ctrl+T` and the theme browser previews themes and applies them live:

![The theme browser](screenshots/themes.png)

The editor changes any role with a color picker while the document restyles behind it. Editing a bundled theme writes a copy, so the shipped files stay as they were. A custom theme is one TOML file dropped in the themes directory.

![The theme editor](screenshots/themes-editor.png)

Nine themes are original designs: `oryx-light` and its dark twin `oryx-dark`, `oryx-sand` and `oryx-night`, `inkstone`, `ember`, `meadow`, `slate`, and `be-vendible`. The rest adapt permissively licensed editor palettes, [credited below](#credits).

## Export to PDF

`Ctrl+Shift+E` opens the export settings: theme, body font and size, code font and size, page size and page numbers. They are kept apart from the app's own appearance and remembered between runs, so reading in a dark theme at 22 points and exporting in a light one at 11 needs no switching back and forth.

`Ctrl+E` exports the document and asks where to save it. The page carries the document as it looks on screen with the configured export theme. Headings become the outline a PDF viewer navigates by, and the fonts are embedded.

**Care is taken so that**:

- Page breaks don't happen through a line,
- Headings are not left alone at the foot of a page,
- Images are not cut,
- Table rows are not split between pages unless the row is taller than the page itself.

## Install

### From a release

Download the archive for your platform from the releases page on [Codeberg](https://codeberg.org/wmahfoudh/oryx/releases) or [GitHub](https://github.com/wmahfoudh/oryx/releases), extract it, and run the installer inside:

```sh
tar -xzf oryx-*-linux-x86_64.tar.gz && cd oryx && ./install.sh
```

On Windows, extract the zip and run `install.ps1` in PowerShell. The installer copies the binary and themes and registers the file association, so markdown files open with Oryx from the file manager. `./install.sh --uninstall` removes everything.

### From source

Building requires **Rust 1.80 or later**.

```sh
git clone https://codeberg.org/wmahfoudh/oryx.git   # or https://github.com/wmahfoudh/oryx.git
cd oryx
make install
```

`make install` builds the release binary, installs it to `~/.local/bin`, copies the themes to `~/.local/share/oryx/themes` and registers the file association. Plain `cargo build --release` works too; the binary looks for `themes/` next to itself, in the XDG data directory, and in the working directory.

> [!TIP]
> Use a release build for everyday reading, because a debug build is noticeably slower on code-heavy documents.

## Use

> [!NOTE]
> There are no menus. **Press `F1`** for the complete shortcut list, `Esc` to close a panel or quit.

```sh
oryx README.md          # open a file
oryx src/main.rs        # code files render highlighted
oryx --theme nord file  # pick a theme for this session
oryx --register         # install the file association and icons
oryx --version          # print the version
```

| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open file |
| `Ctrl+,` | Settings |
| `Ctrl+T` | Theme browser |
| `Ctrl+B` | Folder sidebar |
| `Ctrl+E` | Export to PDF |
| `Ctrl+Shift+E` | Export settings |
| `F1` | Shortcuts help |
| `F5` / `Ctrl+R` | Reload from disk |
| `Ctrl+Plus` / `Ctrl+Minus` | Zoom in / out |
| `Ctrl+0` | Reset zoom |
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy selection as text |
| `Ctrl+Shift+C` | Copy selection as markdown |
| `Ctrl+F` | Find in document |
| `F3` / `Shift+F3` | Next / previous match |
| `Up` / `Down` | Scroll by line |
| `Page Up` / `Page Down`, `Space` / `Shift+Space` | Scroll by page |
| `Home` / `End` | Jump to top / bottom |
| `Escape` | Close overlay or sidebar, quit |

`Ctrl` is `Cmd` on macOS.

## Performance

For a big markdown file, Oryx parses only its first screens before the first paint and the rest arrives from a background thread. Layout shapes on all CPU cores, so the wash-in below the first screens and every zoom or resize is two to three times faster than the previous release. Painting covers a band around the viewport, a couple of screens either side, and scrolling inside that band is a memory copy, so the cost of a scroll frame does not depend on how long the document is. Syntax highlighting and the layout below follow in the background, a slice at a time, without moving anything already on screen. The event loop wakes only for input, and an idle window uses **no CPU at all**.

Measured on a 2019 Linux laptop, release build. First frame is cold launch to first paint; the export column is the export itself, measured after syntax highlighting has settled:

| Document | First frame | PDF export |
|---|---|---|
| 1 MB source file | **80 ms** | 1.0 s |
| 8 MB source file | **85 ms** | 8.2 s |
| 1 MB markdown | **80 ms** | 1.4 s |
| 8 MB markdown | **80 ms** | 9.8 s |

The 8 MB markdown export writes a 9219-page file. A performance test in the repository checks the startup, relayout, paint and export timings.

## Limits

Oryx is built for everyday documents, and some things are out of scope (for the moment):

- It does not edit files.
- Math is drawn as styled text with real symbols, not fully typeset.
- On a file several megabytes long, the colors and the layout below the first screens take a moment to catch up. An export waits for syntax highlighting to finish before it writes, so on the 8 MB file the wall time is roughly double the export column.
- Memory grows with the file. An 8 MB document costs a few hundred megabytes while it is open.
- The implemented HTML is a deliberate subset. No HTML tables, no collapsible sections.
- Remote images ride the operating system's TLS stack, which on Linux means it needs the OpenSSL library (normally shipped with every distro). Without it, badges show placeholders but everything else works.
- macOS compiles but is untested, and there is no packaged build.

## Credits

DejaVu Sans and Courier Prime are embedded in the binary. DejaVu is distributed under the DejaVu Fonts License, Courier Prime under the SIL Open Font License. The settings dialog can switch to any family installed on the system.

<details>
<summary><b>Adapted theme palettes</b> (all MIT, with thanks to their authors)</summary>
<br>

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

</details>

<div align="center">

Oryx is free software, released under the [GNU General Public License v3.0](LICENSE).

</div>

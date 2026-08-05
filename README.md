<div align="center">

# Oryx

**A fast, native viewer for markdown and code.**

*Open it. Read it. Export it. Close it.<br> All in seconds, in one small binary.*

![Platforms](https://img.shields.io/badge/Platforms-Linux%20%7C%20Windows%20%7C%20macOS-blue)
![Version](https://img.shields.io/badge/Version-0.13.1-purple)
![License](https://img.shields.io/badge/License-GPL--3.0-orange)

[Intro](#intro) •
[What it renders](#what-it-renders) •
[Tools](#tools) •
[Themes](#themes) •
[PDF export](#export-to-pdf) •
[Install](#install) •
[Performance](#performance)

![Oryx rendering a markdown document](screenshots/hero.png)

</div>

## Intro

Oryx started as a personal project. I consume a lot of markdown files and did not find a (very) fast tool that could render them beautifully on the desktop without the need for a browser. Exporting to PDF would be a plus. That was the first version of the functional specs. Today Oryx has evolved, adding new features and optimizing performance. Editing markdown is a tempting feature, but not on the roadmap at this stage.

- **Instant**: A document is on screen in well under 100 ms from cold, even an 8 MB file.
- **Light**: Memory stays flat as you scroll, whatever the file size.
- **Distraction-free**: No panes, no toolbars, no menus. `F1` lists the shortcuts, `Esc` closes whatever is open.
- **Beautiful**: 31 themes addressing 51 color roles, for reading and for PDF export alike.
- **Self-contained**: One binary and a folder of themes. No browser engine, no runtime, no GPU requirement.
- **Runs anywhere**: Performs the same on any desktop: a new laptop or an old machine with no graphics card.

## What it renders

The complete recognized syntax, markdown and embedded HTML, is cataloged construct by construct in [SYNTAX.md](SYNTAX.md). The [examples](examples/) folder shows it in whole documents and installs with Oryx.

### Markdown, the whole everyday set

Headings, bold, italic, strikethrough, inline code, links and bare URLs (a link to another file opens it in Oryx), nested blockquotes, horizontal rules, smart quotes and dashes, and emoji shortcodes like `:tada:`. Ordered, unordered and task lists nest as deep as needed, and a wrapped line aligns with the text above it, not with the bullet. Tables keep per-column alignment, shade alternating rows and wrap long cells, so a wide table never runs off the page.

### Code, highlighted

Fenced blocks get a bordered panel and syntax colors for the languages most people write, and a line too long for the panel wraps inside it. Oryx also opens source files directly and renders the whole file as one highlighted document. Over a hundred extensions carry colors, from Rust and Python through TypeScript, Kotlin, Swift, Terraform and Zig, and a `Dockerfile` or a `Makefile` is recognized by its name alone. Any other text file opens in the code font, and a binary is announced in one line.

![Oryx rendering highlighted code](screenshots/code.png)

### GitHub flavor and more

All five GitHub alert kinds are styled, each with its own color and title. A YAML frontmatter header becomes a small metadata panel above the document. Footnote markers sit raised in the text and link to their definitions, gathered at the foot of the document.

**Images and badges** render in place: PNG, JPEG, GIF, WebP or SVG. Remote images are fetched in the background and cached on disk, so a README covered in badges comes up immediately the second time it is opened, and keeps working offline. If a path is broken, it gets a placeholder carrying the alt text.

**Embedded HTML** covers what GitHub renders: tables with or without a header row, collapsible `<details>` sections, HTML headings, lists and quotes, definition lists, centered blocks, images at a set width or height, rows of clickable badges, and the inline tags down to `mark`, `kbd` and `small`. Search sees into a closed section, and jumping to a match unfolds it.

![Oryx rendering a GitHub style README](screenshots/github.png)

### Math, typeset

Oryx typesets TeX math in the STIX Two Math font: real fractions, radicals, matrices, stretched delimiters and stacked limits, not styled text. All four GitHub notations work: `$...$`, `$$...$$`, a `math` fence, and the backtick form ``$`...`$``. Oryx infers whether a dollar sign is a currency or a math delimiter, so prices like $5-$10 stay text.

The command vocabulary follows KaTeX: Greek, binary operators, relations and their negations, arrows, accents, the seven math alphabets, operator names, spacing, the matrix environments, and `\newcommand` macros. A command Oryx does not know renders as its literal source, in place, and the rest of the equation still typesets. An equation wider than the window shrinks to fit. PDF export includes the typeset math, and text copied from the PDF reads back as the equation's characters. The supported commands are listed in [SYNTAX.md](SYNTAX.md#math), and [examples/sample-math.md](examples/sample-math.md) shows everything in one document.

![Typeset math in Oryx](screenshots/math.png)

## Tools

- **Find in document**: `Ctrl+F` searches text. The search is smart about case: `oryx` matches Oryx, ORYX and oryx, while `Oryx` performs an exact match. A match can cross styling, so `fast viewer` is found even when it was written as **fast** *viewer*, and it can cross a wrapped line. The whole document is searchable even while a big file is still loading.
- **Select and copy**: `Ctrl+C` copies a selection as plain text. `Ctrl+Shift+C` copies the original markdown of the selection. A double click selects the word, a triple click the paragraph, the code line or the table cell. Select all is instant at any file size, a selection survives zooming, theme switches and window resizes, and both copies work before a big file has finished loading.
- **Sidebar**: `Ctrl+B` opens a two-tab panel: the folder tree around the open file, and an outline of the document's headings that tracks the reading position, folds its branches, and jumps on a click. Both tabs drive entirely from the keyboard.
- **Open file**: `Ctrl+O` opens the native file dialog.
- **Live reload**: `F5` reloads a file being edited elsewhere.
- **Zoom**: `Ctrl+Plus` (in) and `Ctrl+Minus` (out).
- **Persistence**: Window geometry, the active theme, the sidebar and the last folder are all saved and restored at every start.

## Themes

Thirty-one themes ship with Oryx. Each is a single TOML file with **51 color roles**, so every element can be colored on its own. A missing key falls back to a default; a malformed file is skipped, and the active theme stays.

Press `Ctrl+T` and the theme browser previews themes and applies them live. The arrow keys preview each theme as they step through the list, `Enter` keeps it, and `Escape` restores the previous one:

<p align="center">
  <img src="screenshots/themes.png" alt="The theme browser">
</p>

The editor changes any role with a color picker while the document restyles behind it. Editing a bundled theme writes a copy, so the shipped files stay as they were. A custom theme is one TOML file dropped in the themes directory.

<p align="center">
  <img src="screenshots/themes-editor.png" alt="The theme editor">
</p>

Nine themes are original designs: `oryx-light` and its dark twin `oryx-dark`, `oryx-sand` and `oryx-night`, `inkstone`, `ember`, `meadow`, `slate`, and `be-vendible`. The rest adapt permissively licensed editor palettes, [credited below](#credits).

## Export to PDF

`Ctrl+Shift+P` opens the export settings: theme, body font and size, code font and size, page size, orientation and page numbers. They are kept apart from the app's own appearance and remembered between runs, so reading in a dark theme at 22 points and exporting in a light one at 11 needs no switching back and forth.

`Ctrl+P` exports the document and asks where to save it. The page carries the document as it looks on screen with the configured export theme. Headings become the outline a PDF viewer navigates by, and the fonts are embedded. Emoji render in the PDF as images, so a document full of them exports fine.

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

On Windows, extract the zip and run `install.ps1` in PowerShell. The installer copies the binary, the themes and the example documents, and registers the file association, so markdown files open with Oryx from the file manager. `./install.sh --uninstall` removes everything.

### From source

Building requires **Rust 1.80 or later**.

```sh
git clone https://codeberg.org/wmahfoudh/oryx.git   # or https://github.com/wmahfoudh/oryx.git
cd oryx
make install
```

`make install` builds the release binary, installs it to `~/.local/bin`, copies the themes and examples to `~/.local/share/oryx` and registers the file association. Plain `cargo build --release` works too; the binary looks for `themes/` next to itself, in the XDG data directory, and in the working directory.

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
| `Ctrl+P` | Export to PDF |
| `Ctrl+Shift+P` | Export settings |
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

For a big markdown file, Oryx parses only its first screens before the first paint and the rest arrives from a background thread. Layout shapes on all CPU cores, so the wash-in below the first screens and every zoom or resize uses the whole machine. Only the part of the document around the reading position is kept in drawn form; scrolling rebuilds the landing from recorded positions, identically, in about a millisecond, so memory stays flat however long the document is. Painting covers a band around the viewport, a couple of screens either side, and scrolling inside that band is a memory copy, so the cost of a scroll frame does not depend on how long the document is. Syntax highlighting and the layout below follow in the background, a slice at a time, without moving anything already on screen. A PDF export streams pages to disk as they are laid out, so even a five-thousand-page export runs in a few megabytes of working memory. The event loop wakes only for input, and an idle window uses **no CPU at all**.

Measured on a 2019 Linux laptop, release build. First frame is cold launch to first paint; the export column is the export itself, measured after syntax highlighting has settled:

| Document | First frame | PDF export |
|---|---|---|
| 1 MB source file | **80 ms** | 1.3 s |
| 8 MB source file | **85 ms** | 10.2 s |
| 1 MB markdown | **80 ms** | 1.2 s |
| 8 MB markdown | **80 ms** | 10.7 s |

The 8 MB markdown export writes a 9219-page file. While open, the 8 MB markdown file reads in about 200 MB of memory and the 8 MB source file in about 90 MB. Performance tests in the repository check the startup, relayout, paint and export timings and the memory figures.

> [!NOTE]
> Oryx is not a markdown-to-PDF converter. Its export reproduces the page you read, pixel for pixel: the theme, every shaped glyph, syntax colors for close to a hundred languages, images, links, the outline and the embedded fonts, at a millisecond or two per finished page whatever the document size. Raw conversion without any of that is far faster, a few milliseconds for a whole small file, and it is a different job.

## Limits

Oryx is built for everyday documents, and some things are out of scope (for the moment):

- It does not edit files.
- On a file several megabytes long, the colors and the layout below the first screens take a moment to catch up. An export waits for syntax highlighting to finish before it writes, so on the 8 MB file the wall time is roughly double the export column.
- The implemented HTML is a deliberate subset: what GitHub renders in a README, nothing more.
- Remote images ride the operating system's TLS stack, which on Linux means it needs the OpenSSL library (normally shipped with every distro). Without it, badges show placeholders but everything else works.
- macOS compiles but is untested, and there is no packaged build.

## Credits

DejaVu Sans, Courier Prime and STIX Two Math are embedded in the binary. DejaVu is distributed under the DejaVu Fonts License, Courier Prime and STIX Two Math under the SIL Open Font License. STIX renders the math and stays out of the font picker. The settings dialog can switch the text fonts to any family installed on the system.

<details>
<summary><b>Adapted theme palettes</b> (all MIT, with thanks to their authors)</summary>

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

<details>
<summary><b>Bundled grammars</b> (beyond syntect's defaults, with thanks to their authors)</summary>

- TOML ([sublimehq/Packages](https://github.com/sublimehq/Packages))
- INI ([jwortmann/ini-syntax](https://github.com/jwortmann/ini-syntax), MIT)
- Kotlin ([guille/sublime-kotlin](https://github.com/guille/sublime-kotlin), public domain)
- Swift ([aerobounce/Swift-Next](https://github.com/aerobounce/Swift-Next), MIT)
- TypeScript and TSX, Microsoft's grammars (Apache-2.0) as converted by [bat](https://github.com/sharkdp/bat)
- Dockerfile ([keith-hall/Containerfile-sublime-syntax](https://github.com/keith-hall/Containerfile-sublime-syntax), MIT)
- Zig ([ziglang/sublime-zig-language](https://github.com/ziglang/sublime-zig-language), MIT)
- Terraform and HCL ([alexlouden/Terraform.tmLanguage](https://github.com/alexlouden/Terraform.tmLanguage), MIT)
- GraphQL ([dncrews/GraphQL-SublimeText3](https://github.com/dncrews/GraphQL-SublimeText3), MIT)
- Protocol Buffers ([VcamX/protobuf-syntax-highlighting](https://github.com/VcamX/protobuf-syntax-highlighting), MIT)

Each grammar ships with its license text beside the source under `assets/syntaxes/`.

</details>

<div align="center">

Oryx is free software, released under the [GNU General Public License v3.0](LICENSE).

</div>

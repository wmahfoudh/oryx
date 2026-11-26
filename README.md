<div align="center">

<!-- WORDMARK: the logo and "Oryx" side by side, on a rounded rectangle. wordmark-dark.svg has light text on a dark rectangle (dark mode), wordmark-light.svg has dark text on a light rectangle (light mode). -->
<h1 align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="images/wordmark-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="images/wordmark-light.svg">
  <img src="images/wordmark-dark.svg" alt="Oryx" width="260">
</picture>
</h1>

**Native code and text editor with the fastest Markdown renderer.**<br>
Full-spec Markdown, LaTeX math, and ebook or comic reading on a custom rendering engine  
for Linux, Windows, and macOS.

<a href="https://github.com/wmahfoudh/oryx/releases"><img alt="Release" src="https://img.shields.io/github/v/release/wmahfoudh/oryx?style=for-the-badge&label=RELEASE&color=purple"></a>
<a href="https://www.rust-lang.org"><img alt="Language" src="https://img.shields.io/badge/LANGUAGE-RUST-orange?style=for-the-badge&logo=rust&logoColor=white"></a>
<a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/LICENSE-GPL--3.0-darkgreen?style=for-the-badge"></a>

[Install](#install) · [Features](FEATURES.md) · [Shortcuts](SHORTCUTS.md) · [Changelog](CHANGELOG.md)

<img src="images/oryx.gif" alt="Oryx demo: an 8 MB file, editing, themes, then code, an ebook and a comic">

</div>

Oryx is a daily-driver code and text editor made for a keyboard-first workflow. Files open directly in reading mode, and `Ctrl+E` toggles instantly between reading and editing. With no menus and no toolbars, the interface stays out of your way so you can focus on your code, writing, and reading. As a true native app running its own rendering engine (with no embedded browser or Electron), it combines raw speed with clean typography: showing an **8 MB** Markdown file takes **milliseconds** (try it yourself with the `markdown-challenge.md` file in the [examples](examples/) folder).

## Install

<a href="https://apps.microsoft.com/detail/9NQGHNSJF3VB"><picture><source media="(prefers-color-scheme: dark)" srcset="https://get.microsoft.com/images/en-us%20light.svg"><img alt="Get it from Microsoft" src="https://get.microsoft.com/images/en-us%20dark.svg" width="200"></picture></a>

Packages are on the [releases page](https://github.com/wmahfoudh/oryx/releases):

- **Windows**: the Microsoft Store (badge above), `winget install Steerania.Oryx`, the MSI installer, or the portable zip, which can also install Oryx in your user folder without administrator rights.
- **macOS**: `brew install --cask wmahfoudh/tap/oryx`, or the `.dmg`. The app is not signed, so macOS asks you to approve it once ([how](INSTALL.md#macos)).
- **Debian and Ubuntu**: `sudo apt install ./oryx-editor_*_amd64.deb`
- **Fedora and openSUSE**: `sudo dnf install ./oryx-editor-*.x86_64.rpm` (on openSUSE, `zypper` in place of `dnf`)
- **Arch Linux**: `sudo pacman -U oryx-editor-bin-*.pkg.tar.zst`
- **Any Linux**: the AppImage, or the tarball with its install script.

For detailed installation instructions and for building from source, please check [INSTALL.md](INSTALL.md).

## What Oryx does

### Fast with any file size

Oryx lays out only the first screens on open and processes the rest in the background across CPU cores. Memory stays flat while scrolling because only the visible region is drawn, and an idle window uses zero CPU. A lot of engineering went into making this scale without sluggishness, tested on files up to 200 MB, so Oryx runs smoothly even on an old laptop without a dedicated GPU. I do check Oryx [performance](#performance) before every release.

### Uses the file system

Oryx works directly on your disk: no vault, no library, and nothing to import. Organize your files in plain folders, just as you always have. Open anything from the sidebar, with `Ctrl+O`, by dropping a file or folder onto the window, or from a terminal with `oryx notes.md`. The sidebar displays the current folder alongside an outline of the active file.

Switch to another file in the sidebar and return later to find your exact scroll position preserved. `Alt+Left` and `Alt+Right` navigate backward and forward across your file history or jump between locations in the same document, just like a web browser. Middle-click any file in the sidebar, or press `Ctrl+Enter`, to open it in a separate Oryx window (no tabs for the moment 🙂).

<p align="center"><img src="images/sidebar.png" alt="The Oryx sidebar on a folder of notes, with a note open" width="800"></p>

### All the Markdown, and more

Oryx supports CommonMark, GitHub Flavored Markdown, and Markdown Guide extensions, including tables, footnotes, alerts, definition lists, and abbreviations. It renders GitHub-style HTML such as badges, centered blocks, and collapsible sections, and caches remote images and badges locally so they stay visible offline.

Math support covers TeX, LaTeX, AMS, KaTeX, and MediaWiki syntax, typeset with STIX Two Math and preserved consistently in PDF export. See [SYNTAX.md](SYNTAX.md) for supported constructs and [MATH.md](MATH.md) for math syntax and commands. Press `F1` to open a quick reference covering syntax, math, and shortcuts.

<p align="center"><img src="images/markdown.png" alt="Oryx rendering an alert, a table and typeset math" width="800"></p>

### Themes and PDF export

Oryx includes 34 themes (12 exclusive to Oryx), each defining 51 color tokens so every Markdown and syntax element has distinct styling. Press `Ctrl+T` to browse themes with instant live preview using the arrow keys, `Enter` to apply, or `Escape` to cancel. A theme editor is also included to customize existing themes or create your own with live preview.

`Ctrl+P` exports the current document using the same rendering engine that draws the screen, preserving exact layout, fonts, syntax highlighting, and math, while turning headings into PDF bookmarks. Oryx handles pagination cleanly: it avoids splitting text lines, images, or table rows across page breaks and prevents orphaned headings at the bottom of a page. Export theme, fonts, and page size are configured separately via `Ctrl+Shift+P` (including a `black-and-white` theme for monochrome output), so you can read in a dark theme and export to a light page without switching.

<p align="center"><img src="images/themes.png" alt="The Oryx theme browser open over a document" width="800"></p>

<p align="center"><img src="images/pdf.png" alt="A PDF exported by Oryx" width="800"></p>

### Text and code, for quick edits

Oryx opens any text file and highlights syntax for more than a hundred file extensions, as well as extensionless files like `.bashrc` or scripts starting with a `#!` shebang.

From the terminal, `oryx main.rs:412:10` jumps straight to line 412, column 10, while `git diff | oryx` renders a colored diff directly in the viewer.

Press `Ctrl+E` to enter edit mode with essential code-editing tools: move, duplicate, or delete lines, toggle language-aware comments, run plain or regex find-and-replace, jump to a line, and display line numbers. Saving preserves existing line endings, keeping CRLF and LF files intact.

For everyday text and code editing, Oryx replaces Notepad, Notepad++, and similar editors. While it starts and renders faster than heavyweights like VS Code or Zed, it does not try to match their full IDE feature set: there is no built-in Git client, agentic workflow, or plugin ecosystem.

<p align="center"><img src="images/code.png" alt="Editing code in Oryx with line numbers and regex find and replace" width="800"></p>

### Ebooks and comics

Oryx opens EPUB, MOBI, AZW3 (Kindle) and FB2 books. It shows them in the active theme and fonts instead of the book's own style, and justifies the text like a printed book (`Ctrl+J` turns it off). The sidebar shows the book's table of contents, and Oryx remembers your last reading position. To try it, *The Adventures of Sherlock Holmes* comes with Oryx in the examples folder.

Oryx opens EPUB, MOBI, AZW3 (Kindle), and FB2 books, rendering them with your active theme and fonts rather than the publisher's styling. Text is justified like a printed book (`Ctrl+J` toggles justification if you dont like it), the sidebar displays the table of contents, and the reading position is saved automatically. *The Adventures of Sherlock Holmes* is included in the examples folder to test it out.

For Arabic and Hebrew, Oryx detects text direction per paragraph, so mixed English and RTL passages flow properly on the same page. Amiri (Arabic) and David Libre (Hebrew) are bundled for clean rendering out of the box.

Oryx also reads CBZ and CBR comics. Issues open as a continuous vertical strip suited to webtoons; `Ctrl+Minus` cycles to single-page view and two-page spread view, while `Ctrl+D` reverses page order for manga.

<p align="center"><img src="images/book-arabic.png" alt="An Arabic book in Oryx, justified from right to left" width="800"></p>

<p align="center"><img src="images/comic.jpg" alt="A comic in Oryx, two pages side by side" width="800"></p>

## Keyboard

Oryx has no menus and no toolbars. Everything relies on keyboard and mouse using standard shortcuts like `Ctrl+F` to find, `Ctrl+S` to save, and `Ctrl+B` for bold. There are no modal states: while less expressive than Vim, this modeless flow is easier for most people.

A few consistent rules apply across the app: `Escape` closes the active overlay (or quits Oryx when nothing is open), formatting shortcuts act as toggles, and arrow keys navigate and adjust values inside dialogs. Press `F1` to open a searchable reference of all shortcuts and supported Markdown syntax, or check [SHORTCUTS.md](SHORTCUTS.md) for the full list of shortcuts and CLI flags.

<img src="images/editor-tricks.gif" alt="Editing markdown in Oryx with the keyboard only: lists, tasks, headings, links, pictures, moving lines and find and replace">

| Key | What it does |
|---|---|
| `Ctrl+E` | Switch between reading and editing |
| `Ctrl+O` | Open a file |
| `Ctrl+Shift+B` | Show or hide the sidebar |
| `Ctrl+F` | Find in the document |
| `Ctrl+G` | Go to a line:column |
| `Alt+Left` / `Alt+Right` | Back and forward |
| `Ctrl+T` | Choose a theme |
| `Ctrl+,` | Settings: fonts, sizes, line numbers, word count, autosave, input method |
| `Ctrl+P` | Export to PDF |
| `Ctrl+Shift+P` | PDF export settings |
| `F1` | All shortcuts and the markdown syntax |

On macOS, substitute `Cmd` for `Ctrl`, and use `Cmd+[` / `Cmd+]` to navigate back and forward.

## Is Oryx for me?

Oryx is made for people who want to read and write Markdown quickly from the keyboard in a single window. Files open formatted and themed immediately, even when they are huge. Code editing, ebook reading, and comic viewing follow naturally from the same underlying text and layout engine.

It also fits workflows with LLMs and coding agents, which output large amounts of Markdown. Oryx reloads modified files from disk as soon as the window regains focus, keeping an agent's plan or report formatted and up to date, and you can pipe terminal output directly into it with `llm "your question" | oryx --as md`.

For other needs, other tools might be a better fit:

- Linked notes, a graph of notes, plugins or sync: Obsidian.
- Editing inside the formatted text: Typora or MarkText.
- The source and the preview side by side: Ghostwriter, or VS Code with its preview.
- A full code editor with extensions, git and language servers: VS Code or Zed.
- A library to organize and convert ebooks: Calibre.
- Markdown in the terminal: Glow or Neovim.

## Performance

Every release is benchmarked on my daily machine: a 2019 laptop with an Intel Core i7-8565U, 16 GB of RAM, an NVMe drive, and no dedicated GPU. A release ships only if it matches the previous version's speed, or if any regression has a clear, justified cause. The tables below show measurements from the latest release.

The first table tracks execution time. **First screen** measures the total time from reading the file to displaying the initial view, staying flat at 40 ms regardless of file size. The next three columns cover background work performed while you read: parsing Markdown for the remainder of the document (0 ms for plain source files), laying out the remaining content, and applying syntax highlighting. **PDF export** measures full-document export time.

| File | First screen | Rest read | Rest laid out | Code colors | PDF export |
|---|---|---|---|---|---|
| 1 MB markdown | 40 ms | 29 ms | 0.18 s | 0.68 s | 1.0 s |
| 1 MB source code | 40 ms | 0 ms | 0.11 s | 2.9 s | 0.88 s |
| 8 MB markdown | 40 ms | 246 ms | 1.5 s | 5.9 s | 9.3 s |
| 8 MB source code | 40 ms | 0 ms | 0.85 s | 24.5 s | 8.0 s |

The second table tracks memory usage. **Once loaded** shows steady-state memory after background processing completes, **Peak** shows maximum memory used during loading, and **PDF export** shows the additional memory allocated while exporting.

| File | Once loaded | Highest | PDF export |
|---|---|---|---|
| 1 MB markdown | 22 MB | 34 MB | +10 MB |
| 1 MB source code | 11 MB | 12 MB | +11 MB |
| 8 MB markdown | 172 MB | 255 MB | +34 MB |
| 8 MB source code | 88 MB | 95 MB | +11 MB |

To test large-file performance yourself, the [examples](examples/) folder includes `markdown-challenge.md`, an 8 MB Markdown file of real text.

Books and comics follow the same progressive loading model: initial chapters or pages appear immediately while the rest loads in the background. The third table measures time to first display for ebooks and comics.

| Book | Format | Opens in |
|---|---|---|
| *The Adventures of Sherlock Holmes* | EPUB | 5 ms |
| A book of 300 chapters (1.6 MB) | FB2 | 15 ms |
| A book of 300 chapters (1.6 MB) | MOBI | 9 ms |
| A comic of 40 pages | CBZ | 1 ms |
| The same comic | CBR | 5 ms |

The 211 pages of *The Adventures of Sherlock Holmes* export to PDF in 0.6 s.

My goal is to make Oryx one of the fastest Markdown editors and renderers available. I benchmark Oryx not only across releases, but also against other tools to find and eliminate bottlenecks. The benchmark suite, test files, and results are all tracked in the repository:

```sh
cargo test --release --test perf -- --ignored --nocapture --test-threads=1      # the timings
cargo test --release --test perf_mem -- --ignored --nocapture --test-threads=1  # the memory
```

## Privacy

Oryx requires no account and collects zero telemetry. It never reports what you open, how you use it, or even that it is installed. Network access is used solely to fetch remote images linked inside a Markdown file, such as README badges. Fetched images are cached locally so subsequent opens remain instant, and `oryx --clear-cache` clears the cache. See [PRIVACY.md](PRIVACY.md) for full details.

## Limitations

- Oryx opens text files in UTF-8 only. A file in an older encoding, like Windows-1252 or Shift JIS, is refused rather than shown wrong. Binary files are refused too.
- Books protected by DRM and fixed-layout EPUB books are not supported. CBR comics open when their pages are stored in the RAR archive without compression, which is the usual case.
- Oryx renders the HTML that GitHub allows in a README, plus page breaks and aligned blocks. It does not apply CSS and does not render whole HTML pages.
- The input methods for Chinese, Japanese and Korean are off by default. You turn them on with `Input method` in the settings (`Ctrl+,`). They are not tested on the Mac (for the moment).
- On a file of several megabytes, the rest of the file is laid out and colored in the background, so a few lines can change color a moment after you reach them. A PDF export waits for this coloring to finish.
- On Linux, Oryx needs glibc 2.35 and OpenSSL 3, which recent distributions have ([the list](INSTALL.md#requirements)).
- The Mac build is made on GitHub's Mac machines and tested by some nice Mac users (thank you!).

## Why Oryx?

I read and write Markdown constantly and tried most of the free tools out there. Most open slowly, bog down or crash on large files, or wrap a browser engine or Electron around a fraction of what they actually do. I was thinking of a lightweight desktop app that renders formatted Markdown, opens immediately on any hardware, does not share my notes with somebody on the other side of the planet, and exports clean PDFs. Obsidian came closest, except its vault model got in the way of my workflow.

Because of that, Oryx started as a renderer rather than an editor. Most Markdown tools default to showing raw source first; a browser does not open pages as raw HTML, and a formatted document should not either. Editing followed naturally, along with ebooks and comics once the core engine could handle them. Some ideas were deliberately dropped: adding PDF reading would have cost 5 MB without beating dedicated PDF viewers, and Git or AI agent integrations are unlikely ever to land. Every future feature has to keep Oryx `Fast and Beautiful`.

## Feedback

Bug reports and feature ideas are welcome in the [issues](https://github.com/wmahfoudh/oryx/issues). When reporting a bug, please include your Oryx version (shown in `F1`), your operating system, and, if possible, screenshots alongside the file (or minimal snippet) that reproduces the issue.

## Credits

Oryx bundles five fonts: DejaVu Sans for text, Courier Prime for code, STIX Two Math for math, Amiri for Arabic, and David Libre for Hebrew. DejaVu uses the DejaVu Fonts License, while the other four use the SIL Open Font License. Settings (`Ctrl+,`) let you switch text and code fonts to any font installed on your system if you don't like the defaults.

The sample ebook in [examples](examples/) is the [Standard Ebooks](https://standardebooks.org) edition of *The Adventures of Sherlock Holmes*, in the public domain and dedicated under CC0 by its producers. The 8 MB file in the same directory combines Wikipedia articles under CC BY-SA 4.0 with public-domain books from Project Gutenberg, listed in its final section.

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
- Rosé Pine and Rosé Pine Dawn ([rosepinetheme.com](https://rosepinetheme.com)). `oryx-hero` also uses the accent colors of Rosé Pine.
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

<br>

<div align="center">

Oryx is free software, released under the [GNU General Public License v3.0](LICENSE).

</div>

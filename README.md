<div align="center">

![Oryx: fast editor for markdown and code, reader for ebooks and comics](screenshots/hero.png)

[Installation](#install) | [Themes](#themes) | [Syntax](SYNTAX.md) | [Changelog](CHANGELOG.md) | [GitHub](https://github.com/wmahfoudh/oryx)

<a href="https://github.com/wmahfoudh/oryx/releases"><img alt="Release" src="https://img.shields.io/github/v/release/wmahfoudh/oryx?style=for-the-badge&label=RELEASE&color=purple"></a>
<a href="https://www.rust-lang.org"><img alt="Language" src="https://img.shields.io/badge/LANGUAGE-RUST-orange?style=for-the-badge&logo=rust&logoColor=white"></a>
<a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/LICENSE-GPL--3.0-darkgreen?style=for-the-badge"></a>

<a href="https://apps.microsoft.com/detail/9NQGHNSJF3VB"><picture><source media="(prefers-color-scheme: dark)" srcset="https://get.microsoft.com/images/en-us%20light.svg"><img alt="Get it from Microsoft" src="https://get.microsoft.com/images/en-us%20dark.svg" width="200"></picture></a>

</div>

## Philosophy

Oryx started as a personal project. I work with markdown files and did not find a (very) fast tool that could render them beautifully on the desktop without a browser or an Electron app and without a third party watching my personal notes. Exporting to (a nice-looking) PDF would have been a plus. That was the first version of the functional specs. Oryx has grown a lot since: it became a universal reader and editor, and it stayed fast and beautiful (See [CHANGELOG.md](CHANGELOG.md) for details).

Most markdown editors start by showing the code and then trying to render it. I think the fact that Oryx started as a renderer and not an editor made it something different; after all, nobody develops a browser to start by showing raw HTML. Afterwards, editing and reading ebooks came along the way; it was a consequence. Some ideas like rendering PDF files were tested and rejected for now. PDF reading would have grown the binary by 5.5 MB and would not have added anything better to the community. Features that will remain within `Fast & Beautiful` could be added in the future. Others like git or agentic work integration probably won't. Hopefully, Oryx will remain:  

- **Instant**: A document displays in under 100 ms from cold, even an 8 MB markdown file or a 200 MB ebook.
- **Light**: Memory stays flat however you scroll, whatever the file size. The same speed on a new laptop or an old machine with no graphics card (like mine).
- **Distraction-free**: Keyboard centric, no panes, no toolbars, no menus. `F1` lists the shortcuts, `Esc` closes whatever is open.
- **Beautiful**: 30+ themes, with 51 color roles each, for reading and for PDF export alike.
- **Self-contained**: One binary and a folder of themes and examples. No browser engine, no runtime, no GPU requirement and no need to download themes or syntax highlighting stuff. No account and no telemetry either; what Oryx keeps on your machine is listed in [PRIVACY.md](PRIVACY.md).
- **Opinionated**:
  - Nobody renders justified markdown :smile: (you can turn it of if you don't like it)
  - Most ebook readers do not dare to strip the books' CSS and apply their own.
  - No buttons, no menu bar, fully keyboard driven
  - Embedded default fonts for Latin, Arabic and Hebrew (though you can pick from system fonts) 

## Oryx Scope

Oryx reads CommonMark, the GitHub Flavored Markdown extensions (tables, task lists, strikethrough, footnotes, alerts and math) and the extended syntax listed by the Markdown Guide: heading IDs, definition lists, subscript and superscript, highlight and abbreviations. It also renders the HTML subset GitHub allows in READMEs. [SYNTAX.md](SYNTAX.md) shows every construct twice, as written and as rendered. Ensure you open it with Oryx because some constructs are not supported by GitHub. The [examples](examples/) folder is installed with Oryx and shows the syntax on real documents.

### Markdown

All the standard constructs, plus the extended ones: subscript and superscript (`H~2~O`, `x^2^`), `==highlight==`, definition lists, heading ids in braces, abbreviations with their expansion on hover, and emoji shortcodes like `:tada:` :tada:. Lists nest as deep as needed. A lot of care was given to details: a wrapped line aligns with the text above it, not with the bullet, tables keep per-column alignment, shade alternating rows and wrap long cells, and a link to another file opens it in Oryx.

### Source code

Oryx displays fenced blocks in a bordered panel with syntax colors for code. A code line too long wraps inside the panel. Oryx also opens source files directly and renders them as one highlighted document. Over a hundred extensions are supported, from Rust and Python to Terraform and Zig. Some files like a `Dockerfile` or a `Makefile` are recognized by name. A file without an extension gets its colors when it says what it is: a script with a shebang, a file with an editor modeline, a dotfile like `.bashrc` or `.gitconfig`, or a diff, a JSON, an XML or an INI file recognized by its first lines. A diff shows its added lines in green and its removed lines in red. Any other text file opens in the code font.

Oryx refuses binary files: a file whose first 8 KB holds a zero byte or is mostly unreadable. A text file in an encoding older than UTF-8 is refused too.

![Oryx rendering highlighted code](screenshots/code.png)

### GitHub Flavored Markdown (GFM) and more

The five GitHub alerts are styled, each with its own color and title. Oryx shows a YAML frontmatter header as a small metadata panel above the document. Footnote markers appear raised in the text, numbered in order of use, and link to their definitions, gathered at the foot of the document. The number at the foot links back to the text, and `Alt+Left` (`Cmd+[` on a Mac) returns to where you were reading.

**Images and badges**: Supported formats are PNG, JPEG, GIF, WebP or SVG. Remote images are fetched in the background and cached on disk, so a file with badges comes up immediately the second time it is opened, and keeps working offline. A cached image older than a day is refreshed in the background the next time the file opens. If a path is broken, the image is replaced by a placeholder showing the alt text, or the file name when there is none.

**Embedded HTML** covers what GitHub renders: tables with or without a header row, collapsible `<details>` sections, HTML headings, lists and quotes, definition lists, blocks aligned to the left, the center or the right, images at a set width or height, rows of clickable badges, and the inline tags down to `mark`, `kbd` and `small`. Search sees into a closed section, and jumping to a match unfolds it.

![Oryx rendering a GitHub style README](screenshots/github.png)

### Math

Oryx typesets TeX math in the STIX Two Math font: fractions, radicals, matrices, stretched delimiters and stacked limits. It recognizes all four GitHub notations: `$...$`, `$$...$$`, a `math` fence, and the backtick form ``$`...`$``. Oryx infers whether a dollar sign is a currency or a math delimiter, so prices like `$5-$10` do not mess up rendering.

The command vocabulary is KaTeX compatible: Greek, binary operators, relations and their negations, arrows, accents, the seven math alphabets, operator names, spacing, the matrix environments, and `\newcommand` macros. If Oryx encounters an unknown command, it will render it as its literal source, and the rest of the equation renders normally. An equation wider than the window shrinks to fit (to a reasonable extent). PDF export includes the typeset math, and text copied from the PDF reads back as the equation's characters. The supported commands are listed in [SYNTAX.md](SYNTAX.md#math), and [examples/sample-math.md](examples/sample-math.md) shows many of them in one document.

![Typeset math in Oryx](screenshots/math.png)

## Books

Oryx opens EPUB, FB2, MOBI and AZW3 (Kindle) books and renders them as one continuous document, in the active theme rather than the book's own styling. The book keeps its structure: chapter headings, italics and bold (including the ones its stylesheet sets), images and the cover, tables and highlighted code. The first chapters display immediately and the rest of the book loads in the background. The window title shows the book's title and its format, which helps when the same book exists in several formats. FB2 files zipped as `.fb2.zip` or `.fbz` open too.

Book text is justified: lines end at the same right edge, and the last line of each paragraph stays ragged, like print. `Ctrl+J` turns justification off and on. Markdown files can justify too; they start ragged, and each kind remembers its own choice.

The sidebar's Outline tab shows the book's own table of contents, follows the reading position and jumps on a click. Links inside the book work, so a footnote reference jumps to its note, `Alt+Left` comes back and `Alt+Right` goes forward again. Ebooks reopen where reading stopped, even after Oryx is closed; other files open at the top on a new start.

DRM-protected books and fixed-layout EPUB books are not supported. The [examples](examples/) folder installed with Oryx includes *The Adventures of Sherlock Holmes* to try it on.

### Arabic and Hebrew

Oryx reads right-to-left books. The direction is detected paragraph by paragraph from the text itself (book metadata is often wrong about this), so a book that mixes English and Arabic shows each paragraph on its correct side. Justified Arabic stretches to both edges with the last line of each paragraph ending on the right, the way printed Arabic does. Lists, quotes and headings follow the direction of their text, and selection, search and PDF export work as in any other document.

Two fonts are embedded for this: Amiri for Arabic (a revival of the typeface classical Arabic books were printed in) and David Libre for Hebrew (a digitization of David, a typeface widely used in Hebrew books). Arabic and Hebrew text is rendered in them whatever the selected body font is.

<p align="center">
  <img src="screenshots/rtl-ar.png" alt="An Arabic book in Oryx">
</p>

<p align="center">
  <img src="screenshots/rtl-he.png" alt="A Hebrew book in Oryx">
</p>

If Oryx reads a book's direction wrong, `Ctrl+D` switches it: automatic, right to left, left to right. The choice is remembered for each book.

### Comic books

Oryx opens CBZ and CBR comic book archives and shows the pages in reading order. A comic starts as a vertical strip, every page at the window's width, which reads naturally for webtoons. `Ctrl+Minus` switches to one whole page per screen, and once more to two pages side by side, like an open book; `Ctrl+Plus` goes back up, and `Ctrl+0` shows the whole page from anywhere. In the page views, Up, Down and Space turn pages. For comics that read right to left, like manga, `Ctrl+D` flips the two-page order, and Oryx remembers it for that comic. The outline lists the pages, and Oryx reopens a comic at the page where reading stopped.

Comic book contents are analyzed and files processed accordingly, not by name, so a `.cbr` that is really a `zip` (they are common) works anyway. When an archive is password-protected or damaged, Oryx displays the problem. Compressed `CBR` files are rare and not supported.

## Tools

- **Find in document**: `Ctrl+F` searches text. The search is smart about case: `oryx` matches Oryx, ORYX and oryx, while `Oryx` performs an exact match. A match can cross styling, so `fast viewer` is found even when it was written as `**fast** *viewer*`, and it can cross a wrapped line. The whole document is searchable even while a big file is still loading. The `.*` button in the search bar (or `Alt+R`) switches to regular expressions, in the Rust `fancy-regex` flavor, so capture groups, backreferences and lookarounds are available. `^` and `$` match at line starts and ends, and on the rendered page each block counts as one line; `\n` matches a line break. While a pattern is incomplete, the bar's border changes color instead of showing a match count. Clicking anywhere in the document closes the search bar. The search field behaves like a text box: `Ctrl+Left` and `Ctrl+Right` jump by word, `Shift` selects, `Ctrl+Backspace` and `Ctrl+Delete` delete a word, a double click selects the word and a triple click selects everything. While the field has the keyboard, the document's editing keys stay quiet.
- **Select and copy**: `Ctrl+C` copies a selection with its formatting, so it pastes into an email or a Word document with its headings, lists, tables, quotes, links and code, pictures and formulas included. A terminal or a text editor gets the plain text. `Ctrl+Shift+C` copies the original markdown of the selection. A double click selects the word and lights up every other place the same word appears, a triple click selects the paragraph, the code line or the table cell, and a click with `Shift` held extends the selection to that point. Select all is instant at any file size, a selection survives zooming, theme switches and window resizes, and both copies work before a big file has finished loading.
- **Sidebar**: `Ctrl+Shift+B` shows and hides a two-tab panel: the folder tree around the open file, and an outline of the document's headings that tracks the reading position, folds its branches, and jumps on a click. For a book, the outline is its table of contents. Both tabs drive entirely from the keyboard. The sidebar is open at the first launch, on your home folder, and follows the disk: a file added, removed or renamed shows the next time you touch the window. Files and folders whose name starts with a dot are hidden; `Ctrl+Shift+H` shows them and Oryx remembers your choice. A folder reached through a symbolic link is listed too, and opening it moves the tree to the real folder. A folder Oryx cannot read shows one row saying so, under the `..` row, so you can climb back out.
- **Second window**: a middle click on a file in the sidebar, or `Ctrl+Enter` on the highlighted row, opens it in a second Oryx window, a step down and right of the first (on Wayland your desktop places it). The two windows share the settings and your places in books, and each saves only what it changed.
- **Open file**: `Ctrl+O` opens the native file dialog.
- **Drag and drop**: drop a file on the window to open it, a folder to browse it in the sidebar, or a picture on a markdown file you are editing to add it.
- **Live reload**: Oryx notices when the open file changes on disk and reloads it, as long as there are no unsaved edits. `F5` / `Ctrl+R` reload on demand. If the file is deleted or moved away outside Oryx, a notice says so, the title shows the unsaved dot, and `Ctrl+S` writes the text back.
- **Go to line**: `Ctrl+G` opens a small field. Type a line number and press `Enter`; `412:10` goes to the tenth character of line 412. From a terminal, `oryx main.rs:412` opens the file there.
- **Line numbers**: a `line numbers` row in the settings (`Ctrl+,`) numbers the lines of code and text files, and of a markdown file while you edit it. The rendered page and the PDF stay without numbers. It is off by default.
- **Word count**: a `word count` row in the settings shows words, characters, lines and reading time in the bottom right corner, for the file or for the selection. It counts the text as the page shows it, without the markdown marks. It is off by default.
- **Zoom**: `Ctrl+Plus` (in) and `Ctrl+Minus` (out), or the mouse wheel with `Ctrl` held.
- **Display scale**: Oryx follows the display's scale, so text and controls render at the intended size on a scaled screen (a laptop at 200%, for example). An `interface scale` entry in the settings (`Ctrl+,`) adjusts the size around the detected value, from -50% to +100%, and is remembered.
- **Touch**: On a touch screen, swiping scrolls the document, the sidebar and the dialogs. A swipe released while moving keeps the document scrolling with momentum. Tapping clicks, and a two-finger pinch zooms the document.
- **Persistence**: Window geometry, the active theme, the sidebar and the last folder are all saved and restored at every start. While Oryx is open, switching between files keeps each file's place: a file left mid-edit comes back in the editor, at the same spot, and a code file at the line you were reading.

<p align="center">
  <img src="screenshots/settings.png" alt="Settings dialog in Oryx">
</p>

## Editing

Oryx is now a complete editor, built around the keyboard. For markdown, I think it is one of the best around. For code, it is not meant to compete with VS Code or Zed, which are specialized code editors; Oryx is a good daily driver for quick code edits, note taking and markdown writing, fast, simple and distraction free.

### Edit mode

- `Ctrl+E` enters edit mode, `Escape` (or `Ctrl+E` again) returns to reading. The title says `editing` and a thin line in the selection color runs along the top of the page.
- Code and text files edit on the page itself. A markdown file shows its source, drawn in the theme's colors with the markers visible; `Escape` shows the page again with your edits applied. The line you are on stays at the same height on the screen, both ways.
- An empty file opens in the editor right away. Switching between files during a session keeps each file's place, reading or editing.
- Books cannot be edited, and neither can a file whose text did not read cleanly, since Oryx could not write it back as it was. A notice in the corner says so.

### Writing

- The usual keys: typing, selections, `Ctrl+X` and `Ctrl+V`, `Ctrl+Z` to undo, `Ctrl+Shift+Z` or `Ctrl+Y` to redo. Typing is instant even in very large files.
- `Ctrl+Left` / `Ctrl+Right` jump by word, `Ctrl+Home` / `Ctrl+End` to the ends of the file, `Ctrl+Backspace` / `Ctrl+Delete` delete a word. `Up` on the first line goes to its start, `Down` on the last line to its end.
- With nothing selected, `Ctrl+C` copies the whole line, `Ctrl+X` cuts it, and `Ctrl+V` puts it back as a line above the current one.
- With text selected, a bracket or a quote wraps it instead of replacing it; in a markdown file so do `*`, `_` and a backtick.
- `Alt+Up` / `Alt+Down` move the line or the selected lines, `Ctrl+Shift+D` duplicates them, `Ctrl+Shift+K` deletes them, `Ctrl+/` comments them out. Each is one `Ctrl+Z` to undo.
- `Alt+Left` and `Alt+Right` go back and forward between the places you jumped from, in the editor too.

### Markdown helpers

- `Ctrl+B`, `Ctrl+I` and `` Ctrl+` `` make the selection or the word under the caret bold, italic or code; the same key removes it. With nothing selected: press the key, type, press it again and go on in plain text.
- `Ctrl+K` turns the selection or the word under the caret into a link, and a web address into the link's target.
- `Alt+-`, `Alt+1` and `Alt+X` turn the selected lines into a bullet, numbered or task list, `Alt+.` quotes them, `Ctrl+1` to `Ctrl+6` set the heading level, `Ctrl+L` ticks the task box. A task box can also be ticked with a click on the page, without entering edit mode.
- `Enter` keeps the indentation and continues what you are writing: the next list marker, an unchecked task, the `>` of a quote. `Enter` on an empty item ends the list. `Shift+Enter` adds a line break inside a paragraph or an item.
- `Tab` indents and `Shift+Tab` removes an indent, on every selected line at once. At a list marker, `Tab` nests the item under the one above, inside a quote too; a first item stays where it is, so a list never turns into a code block. A new markdown file indents with four spaces, an existing file keeps what it uses.
- `Ctrl+V` pastes a picture from the clipboard: Oryx saves it as a PNG in an `images` folder next to the file and writes the link, with the caret ready for the description. A picture dropped on the file is added the same way, and one already under the file's folder is linked where it is. A big picture is reduced to 2560 pixels on its longer side; `Ctrl+Shift+V` pastes it as it is.

### Find and replace

- `Ctrl+H` opens a second field under the search box. `Enter` replaces the current match and moves to the next, `Ctrl+Enter` replaces every match at once, and one `Ctrl+Z` brings a replace-all back.
- With regular expressions, the replacement can reuse captured groups: searching `(\w+)/(\w+)` and replacing with `$2/$1` swaps the two sides of every pair. `\n`, `\t` and `\\` in the replacement write a line break, a tab and a backslash.
- The replace field only exists in the editor; the search itself works everywhere.

### Saving

- `Ctrl+S` saves. Lines you did not touch are written back unchanged, and every line keeps its own ending, so a Windows file or an old Mac file stays what it was. A dot next to the file name in the title means unsaved changes. `Ctrl+Shift+S` saves under a new name, while reading too.
- Autosave, off by default, has two rows in the settings (`Ctrl+,`): `save on focus loss` writes the file when you switch to another window, `save after a pause` once you have stopped typing for the time you choose, from 5 seconds to 15 minutes. It never writes over a change another program made on disk: Oryx tells you and waits for your `Ctrl+S`.
- Closing, quitting or reloading with unsaved changes asks first: `S` saves, `D` discards, `Escape` keeps editing. If the file changes on disk while you have unsaved edits, Oryx shows a notice and leaves your edits alone. If the file is deleted or moved away, `Ctrl+S` writes it back.

### New files and notes

- `Ctrl+N` creates a new file: the save dialog opens first, so Oryx knows the type of the file and its syntax colors, then the empty page is ready to type into.
- `Ctrl+M` opens an empty markdown note in the editor, no dialog. The name and the place are chosen at the first `Ctrl+S`; until then the note is unsaved work, and Oryx asks the usual question before closing or opening another file.
- Oryx keeps a copy of the note while you type. If it ends without asking (a crash, a power cut), the next launch offers the note back: `R` recovers it, `D` discards it, `Escape` leaves it for later.

## Themes

Thirty plus themes ship with Oryx. Editable TOML files with **51 color roles**, so every possible markdown element can be colored separately.

Press `Ctrl+T` to open the theme browser. Arrow keys move through the list and preview the selected theme, `Enter` validates and closes the browser, and `Escape` restores the previous one (cancels):

<p align="center">
  <img src="screenshots/themes.png" alt="The theme browser">
</p>

The theme editor changes any color role through a color picker while the document restyles live. Editing a bundled theme creates a copy, so the shipped files remain unchanged. A custom theme is a TOML file saved in the themes directory. Themes are read from `~/.local/share/oryx/themes` (your own and your edited copies) and from the system folders where a package installs them, such as `/usr/share/oryx/themes`. On a Mac, your own themes go in `~/Library/Application Support/oryx/themes`.

<p align="center">
  <img src="screenshots/themes-editor.png" alt="The theme editor">
</p>

Ten themes are original designs: `oryx-light` and its dark twin `oryx-dark`, `oryx-hero`, `oryx-sand` and `oryx-night`, `inkstone`, `ember`, `meadow`, `slate`, and `be-vendible`. The rest adapt permissively licensed editor palettes, [credited below](#credits).

> [!TIP]
> You can drop an existing theme file into Claude Design or Gemini, describe or share a link to something you love and ask it to generate an Oryx-compatible theme.

## Export to PDF

`Ctrl+Shift+P` opens the export settings: theme, body font and size, code font and size, page size, orientation and page numbers. Six page sizes are available: A4, Letter and Legal, and the book trim sizes. When the document is a book, a justify toggle is also present. Oryx separates the export settings from the app's own appearance and remembers them between runs. The idea is that reading in a dark theme at 22 points and exporting in a light one at 11 should not need switching back and forth each time we need an export.

<p align="center">
  <img src="screenshots/pdf-export.png" alt="Oryx export settings">
</p>

`Ctrl+P` exports the document using the configured export settings. After setting your preferences, this is usually the way to go. Markdown headings are converted to PDF outlines, and the fonts are embedded. Emoji render in the PDF as images. A book exports with each chapter starting on a new page, and its table of contents becomes the PDF outline. To force a page break in a markdown file, write `<div style="page-break-after: always"></div>` or `\newpage` on a separate line. The PDF starts a new page there, and the screen shows a dashed line. [SYNTAX.md](SYNTAX.md) shows more ways of forcing a page break.

**During export, Oryx tries to avoid that**:

- Page breaks happen through a line
- Headings are left alone at the foot of a page
- Images are cut
- Table rows are split between pages (unless the row is taller than the page itself)

## Install

### From a release

Oryx is packaged for the following platforms. Pick yours on the [releases page](https://github.com/wmahfoudh/oryx/releases):

- **Debian and Ubuntu**: the `.deb`, `sudo apt install ./oryx-editor_*_amd64.deb`.
- **Fedora and openSUSE**: the `.rpm`, `sudo dnf install ./oryx-editor-*.x86_64.rpm` (or `zypper`).
- **Arch Linux**: the `.pkg.tar.zst`, `sudo pacman -U oryx-editor-bin-*.pkg.tar.zst`.
- **Any Linux**: the AppImage, one file to make executable and run, nothing to install.
- **Windows**: the [Microsoft Store](https://apps.microsoft.com/detail/9NQGHNSJF3VB) (as Oryx Editor), the MSI installer, or the zip with `install.ps1` for an install in your user folder.
- **macOS**: `brew install --cask wmahfoudh/tap/oryx` with Homebrew, or the `.dmg`, one app for Apple Silicon and Intel Macs: open it and drag Oryx to Applications. Either way, the app is not signed with an Apple developer account, so macOS blocks the first open: go to System Settings, Privacy and Security, and click Open Anyway, once. On an older macOS, right-click the app and choose Open.
- **Linux without a package**: the tarball, `tar -xzf oryx-*-linux-x86_64.tar.gz && cd oryx && ./install.sh`; `./install.sh --uninstall` removes it.

The packages are named `oryx-editor` because Arch already ships an unrelated `oryx`, and the Store app is Oryx Editor because the name was taken there; the command is still `oryx` and the app appears as Oryx. A package registers Oryx with the file manager itself, so markdown files and books open with it right away; `oryx --register` is for the tarball and the source install, and says so under a package. If you move from the tarball or `make install` to a package, remove the per-user copy first (`./install.sh --uninstall`): it comes before the package on the PATH and in the launcher.

The Linux packages need glibc 2.35 and OpenSSL 3, which means Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.4 and newer; the AppImage relies on the system's OpenSSL 3 as well.

### From source

Building requires **Rust 1.89 or later**.

```sh
git clone https://github.com/wmahfoudh/oryx.git
cd oryx
make install
```

`make install` builds the release binary, installs it to `~/.local/bin`, copies the themes and examples to `~/.local/share/oryx` and registers the file association. Plain `cargo build --release` works too; the binary looks for `themes/` next to itself, in the XDG data directory, and in the working directory.

> [!TIP]
> Use a release build, because a debug build is noticeably slower on code-heavy documents.

## Using Oryx

After installing, open Oryx from the launcher and browse folders and files through the sidebar. Started without a file, Oryx shows a short page with the basic shortcuts and a tip, a different one each time. You can also drag and drop a file onto the window to open it, or a folder to browse it. On macOS, `Cmd` works wherever the shortcuts below say `Ctrl`, and `Cmd+[` / `Cmd+]` go back and forward in place of `Alt+Left` / `Alt+Right`.

> [!NOTE]
> There are no menus. **Press `F1`** for the complete shortcut list. `Esc` or a click outside closes a dialog, and `Esc` quits.

```sh
oryx README.md          # open a file
oryx book.epub          # books read in the active theme
oryx src/main.rs        # code files render highlighted
oryx notes/             # open the sidebar on a folder
oryx main.rs:412        # open a file at a line
git diff | oryx         # show what a command prints; oryx - reads standard input
git log | oryx --as md  # tell Oryx what kind of text it gets
oryx --theme nord file  # pick a theme for this session
oryx --register         # install the file association and icons
oryx --clear-cache      # remove the downloaded remote images
oryx --version          # print the version
oryx --help             # list these options
```

| Shortcut | Action |
|---|---|
| **Files** | |
| `Ctrl+O` | Open a file |
| `Ctrl+N` | New file |
| `Ctrl+M` | New markdown note; the name and the type are chosen when saving |
| `Ctrl+S` | Save (editing) |
| `Ctrl+Shift+S` | Save as |
| `F5` / `Ctrl+R` | Reload from disk |
| `Ctrl+Shift+R` | Reload and refetch remote images |
| **Navigation** | |
| `Up` / `Down` | Scroll by line, or move the sidebar selection |
| `Page Up` / `Page Down`, `Space` / `Shift+Space` | Scroll by page |
| `Home` / `End` | Jump to top / bottom |
| `Alt+Left` / `Alt+Right` | Go back after a jump (a link, a search hit, a line, another file), and forward again; `Cmd+[` / `Cmd+]` on a Mac |
| `Ctrl+G` | Go to a line |
| `Ctrl+Shift+B` | Show or hide the sidebar (files and outline) |
| `Ctrl+Shift+H` | Show or hide the dot files in the sidebar |
| `Left` / `Right` | Move to the sidebar / to the document |
| `Ctrl+Tab` | Toggle the sidebar tab |
| **Find** | |
| `Ctrl+F` | Find in document |
| `F3` / `Shift+F3` | Next / previous match |
| `Alt+R` | Regex matching on/off |
| `Ctrl+H` | Find and replace (editing only) |
| `Ctrl+Enter` | Replace all (replace open); in the sidebar, open the file in a second window |
| **Selection** | |
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy the selection with its formatting, or the line (editing) |
| `Ctrl+Shift+C` | Copy selection as markdown |
| **Edit** | |
| `Ctrl+E` | Edit the document |
| `Ctrl+X` / `Ctrl+V` | Cut / paste, the line with nothing selected (editing) |
| `Ctrl+Shift+V` | Paste a picture without resizing it (markdown editing) |
| `Shift+Enter` | Line break inside a paragraph or a list item (markdown editing) |
| `Ctrl+Z` | Undo the last edit |
| `Ctrl+Shift+Z` / `Ctrl+Y` | Redo an undone edit |
| `Ctrl+B` / `Ctrl+I` | Bold / italic around the selection or the word, again to remove (markdown editing) |
| `` Ctrl+` `` | Inline code around the selection or the word, again to remove (markdown editing) |
| `Ctrl+K` | Link around the selection or the word; pasting an address over a selection links it too (markdown editing) |
| `Alt+-` | Bullet list on the selected lines, again to remove (markdown editing) |
| `Alt+1` | Numbered list on the selected lines, again to remove (markdown editing) |
| `Alt+X` | Task list on the selected lines, again to remove (markdown editing) |
| `Alt+.` | Quote the selected lines, again to remove (markdown editing) |
| `Ctrl+1` to `Ctrl+6` | Heading level of the line, the same level again to clear it (markdown editing) |
| `Ctrl+L` | Tick or untick the task box of the line (markdown editing) |
| `Alt+Up` / `Alt+Down` | Move the line or the selected lines up / down (editing) |
| `Ctrl+Shift+D` | Duplicate the line or the selected lines (editing) |
| `Ctrl+Shift+K` | Delete the line or the selected lines (editing) |
| `Ctrl+/` | Comment or uncomment the line or the selected lines (code and markdown editing) |
| **View** | |
| `Ctrl+T` | Choose a theme |
| `Ctrl+,` | Settings: fonts, sizes, interface scale, line numbers, word count and autosave |
| `Ctrl+Plus` / `Ctrl+Minus` | Zoom in / out; in a comic, switch between page views |
| `Ctrl+0` | Reset zoom; in a comic, show the whole page |
| `Ctrl+J` | Justify prose (markdown and books) |
| `Ctrl+D` | Reading direction: automatic, right to left, left to right |
| **Export** | |
| `Ctrl+P` | Quick export to PDF |
| `Ctrl+Shift+P` | Define export settings, then export |
| **Help** | |
| `F1` | Open the help page, and close it |
| `Escape` | Close overlay, clear the selection, leave editing, quit |

`Ctrl` is `Cmd` on macOS.

## Performance

Performance is one of the reasons Oryx exists. Many markdown viewers start struggling above one megabyte, and lose the nice look on the way. Oryx keeps the same reading experience whatever the file size, on a new laptop or on an old machine with no graphics card.

Here is how: Oryx parses only the first screens of a big file before the first paint, and the rest comes from a background thread. The layout uses all CPU cores, for the rest of the file as well as for every zoom or resize. Only the part of the document around the reading position is kept drawn, so memory stays flat however long the document is. Syntax colors and the layout below arrive in the background, a slice at a time, without moving what is already on the screen. A PDF export writes pages to disk as they are laid out, so even a five thousand page export runs in a few megabytes. A book opens the same way: the first chapters first, the rest in the background, images included. An idle window uses **no CPU at all**.

The numbers below come from the last phase gate, release build, on a 2019 Linux laptop with no graphics card; every gate measures them again before a release. Open is the time before the first screen shows, held to 40 ms whatever the file. Parse is the rest of the markdown read, Highlight the syntax colors of the whole file, Full pass the layout of the whole file, all three in the background while you read. PDF export is the export of the whole file.

| File | Open | Parse | Highlight | Full pass | PDF export |
|---|---|---|---|---|---|
| 1 MB markdown | 40 ms | 31 ms | 0.7 s | 0.18 s | 0.96 s |
| 1 MB source file | 40 ms | 0 ms | 2.8 s | 0.11 s | 0.9 s |
| 8 MB markdown | 40 ms | 262 ms | 5.8 s | 1.5 s | 9.4 s |
| 8 MB source file | 40 ms | 0 ms | 24.1 s | 0.86 s | 8.1 s |

Memory: Settled is what the file takes once everything is loaded and laid out, Peak the most it takes on the way there, Export the extra during a PDF export.

| File | Settled | Peak | Export |
|---|---|---|---|
| 1 MB markdown | 22 MB | 34 MB | +10 MB |
| 1 MB source file | 11 MB | 12 MB | +11 MB |
| 8 MB markdown | 173 MB | 257 MB | +34 MB |
| 8 MB source file | 88 MB | 95 MB | +11 MB |

Books open the same way. *The Adventures of Sherlock Holmes* (EPUB) shows its first chapters in 4 ms and exports its 211 pages in 0.8 s; a 300-chapter FB2 opens in 27 ms, a MOBI in 14 ms, a 40-page CBZ in 1 ms and the same comic as CBR in 7 ms. The performance tests in the repository check these timings and the memory figures.

> [!NOTE]
> Oryx is not a markdown-to-PDF converter. Its export reproduces the page you read: the theme, every glyph, syntax colors for close to a hundred languages, images, links, the outline and the embedded fonts, at a millisecond or two per finished page whatever the document size. Raw conversion without any of that is a different, far faster job: a few milliseconds for a whole small file.

## Limitations

- On a file several megabytes long, the layout below the first screens takes a moment to catch up. Syntax colors appear right away wherever you are reading, and a few lines can change color a moment later, once the full pass reaches them. An export waits for syntax highlighting to finish before it writes, so on a big file the wall time is about the Highlight and the PDF export columns above added together.
- Embedded HTML is a subset: the tags GitHub allows in a README, plus a few extras like aligned blocks and page breaks. Oryx does not render CSS or a whole HTML page.
- Editing types in any keyboard layout, but Chinese, Japanese and Korean input methods are not supported.
- Remote images use the operating system's TLS stack, which on Linux means it needs the OpenSSL library (normally shipped with every distro). Without it, badges show placeholders but everything else works.
- The Windows release is compiled on my Linux machine, and the macOS build on GitHub's Mac machines. I don't have a Mac, so the Mac build is tested by other people, not by me every day.

## Credits

DejaVu Sans, Courier Prime, STIX Two Math, Amiri and David Libre are embedded in the binary. DejaVu is distributed under the DejaVu Fonts License, the other four under the SIL Open Font License. STIX renders the math and stays out of the font picker; Amiri renders Arabic and David Libre renders Hebrew. The settings dialog can switch the text fonts to any family installed on the system.

The sample book in [examples](examples/) is the [Standard Ebooks](https://standardebooks.org) edition of *The Adventures of Sherlock Holmes*, in the public domain and dedicated with CC0 by its producers.

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

<br>

<div align="center">

Oryx is free software, released under the [GNU General Public License v3.0](LICENSE).

</div>

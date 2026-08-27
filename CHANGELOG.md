# Changelog

## v1.0.0

- Oryx ships as a `.deb` and an `.rpm` on the release page, beside the tarball, the zip and the MSI.
- An AppImage is on the release page too: one file that runs on any Linux with glibc 2.35 and OpenSSL 3, nothing to install.
- On Arch Linux, Oryx is in the AUR as `oryx-editor` (built from source) and `oryx-editor-bin` (the release binary).
- Headings and bold words stay in the chosen body font when that font has no bold face (a cursive or display font, for example). Before, they switched to another font, on screen and in the PDF.
- The Linux binary is built on Ubuntu 22.04 and runs on Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.4 and newer (glibc 2.35, OpenSSL 3).
- The Windows installer is 9 MB instead of 26: the old one carried the program twice, once as the file and once as the source of its icon.
- On Linux, Oryx registers itself as `com.steerania.Oryx`, so the window icon shows on Wayland desktops; an earlier registration is cleaned up. Under a package, `oryx --register` says the package already did it.
- Themes are also found in the system folders where a package installs them (`/usr/share/oryx/themes` and the other `XDG_DATA_DIRS` entries).
- Clicking a heading in the outline goes to that heading, not to the first one with the same text. Repeated headings get numbered anchors as on GitHub (`#title`, `#title-1`).
- A privacy page, `PRIVACY.md`: no account, no telemetry, what is kept on the machine and where.
- The `F1` page links to the documentation on GitHub, the project's home.

## v0.15.8

A review of the whole program before publishing it wider: two dozen fixes and small additions, plus two more reported while testing. The six fixes that could lose work or hide content come first.

- A file with a very long line (a minified script, a one-line JSON) no longer freezes Oryx. A 250 KB line went from 13.8 seconds to 53 milliseconds to open.
- Reloading a file that got shorter while it was being edited no longer crashes; the caret stays inside the file.
- Saving keeps the file's permissions, so a script stays executable.
- HTML comments, and the other markup a browser never shows, are hidden on the page, including the commented-out badge in a README.
- A file starting with a byte order mark shows no stray character, and the mark is preserved on save.
- HTML entities (`&nbsp;`, `&copy;`, `&mdash;`, numeric codes and about 130 named ones) are decoded inside HTML blocks.
- Started without a file, Oryx shows a short page with the basic shortcuts, and `oryx --help` prints the usage.
- `oryx FOLDER` opens the sidebar on that folder. A file dropped onto the window opens, and a dropped folder opens the sidebar on it, on Wayland as well as X11, Windows and macOS.
- The mouse wheel zooms with `Ctrl` held.
- On macOS the shortcuts respond to `Cmd` as well as `Ctrl`.
- On Linux, "Open with Oryx" appears for Kindle, FB2 and comic files too, right after installing, without logging out.
- Save As keeps the caret where it was, and the old file reopens in read mode.
- Remote images older than a day are refreshed in the background. `Ctrl+Shift+R` reloads the page and fetches its remote images again; `oryx --clear-cache` empties the image cache.
- Three crashes on malformed books are fixed; each shows an error message instead.
- The settings file is written safely, and a value edited out of range by hand is corrected instead of breaking the display.
- A folder reached through a symbolic link appears in the sidebar; clicking it moves the tree there.
- Pictures in a markdown file now share the memory budget books already had, and a camera photograph is stored no larger than 4096 pixels on its long side: a page of twelve 24-megapixel photos holds 107 MB instead of 1,177 MB.
- A missing inline image shows its alt text, or its file name, inside its box.
- `Escape` clears the selection before it quits.
- The zoom shortcuts work on every keyboard layout, AZERTY included.
- The dependencies are audited before each release; the two advisories the first audit found are gone from the build.
- On Windows, cancelling the export dialog with `Escape` no longer closes Oryx with it.

## v0.15.7

Oryx now reads Arabic and Hebrew books, right to left, in fonts made for them.

- The direction is detected paragraph by paragraph from the text itself, so a book mixing English and Arabic shows each paragraph on its correct side. Justified Arabic stretches to both edges with the last line ending on the right, like print.
- Two fonts are embedded: Amiri for Arabic, a revival of the typeface classical Arabic books were printed in, and David Libre for Hebrew. They render their scripts whatever body font is selected.
- Lists, quotes and headings follow the direction of their text: bullets, numbers and quote bars move to the right side of a right-to-left block.
- Selecting, copying and searching work in right-to-left text as in any other, and the PDF export keeps the direction, the fonts and copyable text.
- `Ctrl+D` switches the reading direction (automatic, right to left, left to right) and is remembered per book. In the two-page comic view it flips the page order, for manga.
- Tested against the full library, now including every EPUB: 2,689 of 2,697 books opened and read correctly in 75 seconds (the eight failures are broken downloads, damaged files, or fixed-layout books, each refused with a clear message).
- The two fonts add about 1.1 MB to the binary; the whole feature beyond them is about 22 KB of code and no new external libraries.

## v0.15.6

Oryx now reads comic books, and `Alt+Left` goes back after any jump.

- CBZ and CBR comics open with their pages in order. A comic starts as a vertical strip at the window's width; `Ctrl+Minus` switches to one whole page per screen, then to two pages side by side like an open book, `Ctrl+Plus` goes back up, and `Ctrl+0` shows the whole page from anywhere. In the page views, `Up`, `Down` and `Space` turn pages.
- Comics are opened by their content, not their file name, so a `.cbr` that is really a zip works anyway. Password-protected and damaged archives are refused with a clear message, and so is the rare CBR with compressed pages (almost all store them, which Oryx reads).
- `Alt+Left` goes back after clicking a link, a footnote or an outline entry, in every kind of document, one jump at a time.
- Fixed: a reading position saved at the very top of a markdown file was silently lost.
- Comics open fast whatever their size: a 78 MB, 192-page book opens in under 50 ms. Tested against a real library: 240 of 242 books and comics opened correctly in under 9 seconds (the two failures are damaged files known from the last release).
- The RAR reading was written from scratch for Oryx and checked byte for byte against the reference unrar tool. All of this adds about 52 KB to the binary and no new external libraries.

## v0.15.5

Oryx now reads three more book formats: FB2, MOBI and AZW3 (Kindle). Every format gets the same treatment as EPUB: the active theme, the outline panel, working links and footnotes, remembered reading positions, and PDF export with chapters and bookmarks.

- FB2 books open with their pictures, poems and footnotes. Files saved as `.fb2.zip` or `.fbz` work too, and so does the windows-1251 encoding common in Russian books.
- MOBI and AZW3 books open with their pictures, chapter links and table of contents. A file carrying both the old and the new Kindle format inside uses the newer one. DRM-protected books are refused with a clear message, and a damaged file says it is damaged instead of something cryptic.
- The window title shows the book's format next to its title, so the same book in EPUB and in MOBI can be told apart.
- Reading positions are remembered per format: leaving the MOBI of a book does not move your place in its EPUB.
- Tested against a real library: 231 of 233 FB2, MOBI and AZW3 files opened correctly, in 10 seconds total (the two failures are damaged files). The same book costs about the same to open whichever format it comes in.
- All of this adds about 150 KB to the binary and no new external libraries; the Kindle format support was written from scratch for Oryx.

## v0.15.4

Oryx can now edit what it reads. Built in four milestones; books stay read-only.

**Milestone 4, find and replace**

- Search can use regular expressions: press the `.*` button in the search bar, or `Alt+R`. Capture groups, backreferences and lookarounds all work (the Rust `fancy-regex` flavor). `^` and `$` match line by line, and on the rendered page every block counts as a line. Works while reading too.
- `Ctrl+H` opens find and replace in the editor. `Enter` replaces the current match and moves to the next; `Ctrl+Enter` replaces every match at once, as a single undo step. A replacement can reuse captures: searching `(\w+)/(\w+)` and replacing with `$2/$1` swaps every pair in the file.
- The search boxes behave like real text boxes now: their own undo and clipboard, a monospace font, and `Ctrl+Home`, `Ctrl+End` and the word jumps pass through to the document. Clicking anywhere else closes the bar, like the dialogs.
- Editing is visible at a glance: the title says `editing`, a thin line in the theme's selection color runs along the top of the page, and the unsaved marker is now a dot before the file name instead of an asterisk.
- Fixed on the way: searching `f` could highlight `fi` (the font draws them as one shape; the highlight now splits it), and a search for an empty pattern could hang.

**Milestone 3, everyday editing habits**

- `Enter` keeps the line's indentation, and in a markdown file it continues what is being written: list markers repeat, numbered lists count on, task items continue unchecked, quotes keep their `>`, and `Enter` on an empty item ends the list.
- `Tab` indents and `Shift+Tab` removes an indent, on every line of a selection at once. With the caret at a list marker, `Tab` nests the item. Whether `Tab` writes a tab or spaces follows what the file already uses.
- Task checkboxes tick by a click on the rendered page, without entering the editor. Nothing else on the page moves, `Ctrl+Z` undoes it (undo now works while reading too), and `Ctrl+S` saves it.
- `Escape` quits without closing the sidebar first; hiding stays on `Ctrl+Shift+B`, and the sidebar comes back the way it was left.
- The dialogs improved too: a title band colored from the theme, and a click outside closes any of them.
- Fixed: opening the help page while editing a markdown file blanked the outline to "No headings" for the rest of the edit.

**Milestone 2, markdown editing**

- Markdown files can be edited. `Ctrl+E` shows the file's own markdown in the editor, `Escape` returns to the rendered page. Everything the editor already does applies: typing, selection, undo, save, the byte guarantee.
- The markdown source is drawn in the theme's own colors: headings in their ramp, `**bold**` in the bold color with its markers, inline code, links, quotes and rules likewise, with real bold and italic faces that keep the columns aligned.
- The editor opens at the passage being read and the page returns to the line being edited, in both directions.
- Coming back without changing anything is instant at any file size; the page is kept, not rebuilt (measured flat at 8 MB).
- While editing, the outline keeps the page's headings, a click moves the caret to that heading, and a save refreshes the list.
- Switching between files keeps each file's place for the session: a file left mid-edit reopens in the editor at its caret, a file left reading reopens at the same passage.
- The `F1` page names the running version and links to the full documentation on Codeberg and GitHub.

**Milestone 1, plain text editing, and fast coloring of huge files**

- Editing begins. `Ctrl+E` edits code and plain text files on the page itself: typing, `Enter`, `Tab`, deletion, selection with `Shift` and the mouse, cut, copy and paste. `Escape` returns to reading. Markdown editing comes in a later version; books stay read-only.
- Undo with `Ctrl+Z`, redo with `Ctrl+Shift+Z` or `Ctrl+Y`. Typed runs undo together; a pause or a jump starts a new step.
- `Ctrl+S` saves. A save never changes a byte you did not touch, line endings included, and a test suite proves it on every build.
- Typing is instant at any file size: a keystroke in an 8 MB file cost about 2 seconds of relayout during development, under 20 ms now.
- `Ctrl+N` creates a new file, `Ctrl+Shift+S` saves under a new name. Closing, quitting or reloading with unsaved changes asks first: `Enter` saves, `D` discards, `Escape` cancels.
- The file reloads by itself when it changes on disk; with unsaved edits a corner notice reports the change instead.
- The caret moves like in a text editor: `Ctrl+Left` / `Ctrl+Right` jump by word, `Ctrl+Home` / `Ctrl+End` jump to the ends, `Ctrl+Backspace` / `Ctrl+Delete` delete by word.
- `F1` opens a help page rendered by Oryx itself: searchable, themed, scrollable. `F1` or `Escape` returns exactly where you were, unsaved edits and all. The old panel retires.
- Markdown text can justify like a book: `Ctrl+J`, remembered separately for markdown (off unless you ask) and books (on). The PDF export follows the same choice.
- Code files draw edge to edge with the theme's code background as the page, no frame around the file.
- Text files show every line the file has, blank lines included, like an editor.
- The title bar carries `*` while there are unsaved edits.
- Shortcut moves: the sidebar toggle is `Ctrl+Shift+B`, the sidebar tab switch is `Ctrl+Tab`.
- Syntax colors reach the part of a big code file you are looking at right away. Jumping to the end of an 8 MB file used to show plain text for about 36 s while the coloring caught up; it now colors in about 9 ms wherever you are.
- Editing a big code file no longer re-colors the whole file after every pause: an 8 MB file took about 34 s, now about 60 ms.
- `End` and `Ctrl+End` reach the real end of a file still being laid out: the view settles there once the whole document is placed, unless you scroll somewhere else meanwhile.
- Fix: exporting a very large markdown file to PDF had doubled in time (about 19 s instead of 10 s on an 8 MB file); caught by the new per-phase performance check and fixed.

## v0.14.3

- Oryx now follows the display scale: on a scaled screen (a laptop at 200%, for example) text and controls appear at their intended size.
- A new `interface scale` entry in the settings (`Ctrl+,`) adjusts the size around the detected value, from -50% to +100%, and is remembered.
- Touch screens work: swiping scrolls (with momentum in the document), tapping clicks, and a two-finger pinch zooms.
- The `F1` panel fits small windows: a list taller than the screen scrolls.

## v0.14.2

- Book text is justified like print. `Ctrl+J` turns it off and on, and the export settings get their own justify toggle (books only).
- Added 3 new book sizes in addition to A4, Letter and Legal: A5, 6 x 9 in and 5 x 8 in.
- `Left` / `Right` switch between the document and the sidebar, `Ctrl+Left` / `Ctrl+Right` switch the sidebar tab (Files/Outline).
- In the theme browser, `Enter` chooses the theme and closes in one press. `Escape` still cancels.
- The `F1` list is grouped by category with clearer wording, in a more compact panel.

## v0.14.1

- Books use far less memory: images load as the reading reaches them. An 11 MB book full of screenshots drops from 543 MB to 154 MB while open.
- A book opens without decoding a single image, and the page no longer shifts as images arrive.
- Scrolling fast into a new chapter can leave an image blank for a fraction of a second.
- Fix: EPUB 2 books show their real table of contents again (most were silently falling back to a scan of the chapter headings).
- Fix: outline entries stay on one line, and clicking an entry lands on its chapter in every book.
- Windows gets an MSI installer beside the zip, unsigned (for the moment), so the unknown publisher warning shows once.

## v0.14.0

- Oryx opens EPUB books as one continuous document in the active theme: chapter headings, italics and bold, images and the cover, tables and highlighted code.
- Books open instantly: the first chapters are on screen at once (9 ms of parsing for the sample book) and the rest loads in the background, images included.
- The sidebar's Outline tab shows the book's table of contents, follows the reading position and jumps on a click. Links and footnotes inside the book work.
- Oryx remembers where reading stopped in each book and reopens there.
- A book exports to PDF with each chapter on a new page, and its table of contents becomes the PDF outline. The sample book exports its 211 pages in 0.8 s.
- DRM-protected books and fixed-layout books are not supported.
- *The Adventures of Sherlock Holmes* (the Standard Ebooks edition, public domain) ships in the examples folder.
- The file association covers `.epub`, so books open with Oryx from the file manager.
- The binary grows about 1.2 MB for the EPUB machinery.

## v0.13.1

- The export shortcuts moved: `Ctrl+P` exports, `Ctrl+Shift+P` opens the export settings. `Ctrl+E` is reserved for the editing mode planned later.
- The PDF export has an orientation setting: portrait or landscape, remembered.
- In the export settings, clicking the Export row starts the export (before, only Enter worked).
- In the theme browser, the arrow keys preview themes as they move through the list. Enter validates, Escape cancels.
- The sidebar tab headers are no longer painted over when a long outline scrolls.
- The README got a new hero image, a cleaner top section, and plainer wording overall.

## v0.13.0

- Math is really typeset: fractions, roots, superscripts and subscripts, big operators with their limits, and stretching delimiters, in the STIX Two Math font.
- All four GitHub math notations work: `$...$`, `$$...$$`, the ` ```math ` fence, and inline `` $`...`$ ``.
- The command coverage follows KaTeX: Greek letters, arrows, relations, accents, the math alphabets like `\mathbb{R}`, operator names, matrices, `cases` and `aligned`.
- `\newcommand` defines macros with arguments, usable in the same equation.
- If Oryx encounters a command it does not know, it renders it as its literal source, and the rest of the equation still typesets.
- Oryx infers whether a dollar sign is a currency or math, so prices like `$5-$10` stay text.
- A wide equation shrinks to fit the page, and a tall one inside a sentence makes room for itself.
- PDF export includes the typeset math, and text copied from the PDF gives back the equation's characters.
- An `examples` folder ships with the install, and `sample-math.md` shows the math in one document.
- The binary grows about 1MB for the math font and engine.

## v0.12.0

- The sidebar has two tabs now: the file tree and a document outline. The outline follows the scroll position, folds, and a click jumps to the heading.
- Syntax colors for TOML, INI, Kotlin, Swift, TypeScript, TSX, Zig, Terraform, GraphQL and Protocol Buffers. `Dockerfile` and `Makefile` are recognized by name. The binary grows 110KB.
- Collapsible `<details>` sections work like on GitHub. Search finds text in a closed section and opens it when jumping to a match.
- More embedded HTML: tables, headings, lists, quotes, code blocks, definition lists, and the remaining inline tags (`mark`, `small`, underline, `kbd`).
- Double click selects a word, triple click a paragraph.
- A link to another file opens it in Oryx, and a `#section` in the link lands on that section.
- SYNTAX.md in the repository lists what Oryx recognizes.

## v0.11.0

- Memory no longer grows: only the document around the reading position is kept drawn. The 8MB test file drops from 409MB to 200MB while open, a source view from 418MB to 88MB.
- PDF export streams pages to disk as they are laid out: the 8MB code file writes a 5054-page PDF through about 8MB of working memory (it was 228MB).
- Select all is instant at any file size, and both copies work even while a large file is still loading.
- A selection survives zooming, theme switches and window resizes.
- Find covers the whole document while it is still loading, and a phrase now matches across a wrapped line.
- Copying a document as markdown no longer stops at the footnote definitions.
- Documents with emoji export again: emoji embed as images in the PDF.

## v0.10.0

- Big markdown files open instantly: Oryx parses only the first screens before the first paint and the rest arrives in the background. An 8MB file drops from about 440ms to 80ms.
- Layout uses all CPU cores: the coloring below the first screens, zoom and resize are two to three times faster.
- PDF export of the 8MB test file drops from 17s to under 10s.
- Select all on a huge file no longer freezes the app, and a selection survives while syntax colors are still arriving.

## v0.9.0

- The binary got smaller.
- Big files stay fast: hovering, scrolling and search are instant even on an 8MB document.
- PDF export fixes: system fonts embed correctly, images and code blocks are no longer cut between two pages, tall table rows paginate properly.
- Badges can now load behind a proxy on Windows, and a corrupted image cache repairs itself.
- Dracula is the new default theme, and Oryx recreates it if no theme is available.
- The theme browser lists light themes first, then dark ones.

## v0.8.0

- Added PDF export.

## v0.7.1

- Close to a hundred extensions get syntax colors. Unknown text opens in the code font, and binary files are refused.
- Sidebar shows a type icon per file, and its edge drags to resize. Its state, width and last folder are restored on launch.
- New app icon.

## v0.7.0

- Lazy layout: big files open faster.
- Fixed a tables bug.

## v0.6.1

- Lazy syntax highlighting: opening an 8MB markdown file drops from 8686ms to 397ms.
- The app shows a friendly name on Windows.

## v0.5.1

- Fixed the console window that used to open alongside Oryx on launch.
- Improved the oryx-light theme.

## v0.5.0

- Added text search.
- The window size and location are remembered.
- Fixed a bug where remote images did not show after a network hiccup. Oryx now retries twice in the background.

## v0.4.0

- Oryx renders the common markdown set: syntax highlighted code, tables, alerts, footnotes, math literals, remote badges, and embedded HTML.
- Thirty themes and a folder sidebar are included.
- F1 shows the available shortcuts, we have no menus and no buttons.

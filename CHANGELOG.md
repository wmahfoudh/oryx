# Changelog

## v1.2.1

### New features

#### macOS

- Oryx can be installed on a Mac with Homebrew: `brew install --cask wmahfoudh/tap/oryx`.

### Fixes

- In a book, the outline now highlights the chapter you are reading. Before, in some books, the wrong entry was highlighted.
- `Ctrl+G` now shows the line in the middle of the window. Before, the line was at the very top.
- `Alt+Left` and `Alt+Right` now bring you back to the same view you left. Before, the line went to the top of the window.
- In the editor, the current line number is now shown in a small box, so it is easier to see.
- On Windows, the sidebar can now reach your other drives: `..` at the top of a drive shows the list of drives. Before, the sidebar could not leave the drive it started on.

## v1.2.0

### New features

#### Copy

- `Ctrl+C` on the page copies the selection with its formatting, so it pastes into an email or a Word document with its headings, lists, tables, quotes, links and code. Pictures and formulas travel inside the copy as images; word processors and desktop mail programs show them, some web mail does not. A terminal or a text editor still gets the plain text. In the editor, `Ctrl+C` copies the text only.

#### macOS

- A macOS build, one app for Apple Silicon and Intel Macs, shipped as a disk image. It is not signed with an Apple developer account, so the first open is blocked. Allow it once in System Settings, Privacy and Security, Open Anyway.
- On a Mac, `Cmd+[` goes back and `Cmd+]` forward, since `Option+Left` and `Option+Right` move by a word there. The quick reference and the tips show these keys on a Mac.

#### Arch Linux

- The release page has a package for Arch Linux, to install with `sudo pacman -U oryx-editor-bin-*.pkg.tar.zst`.

#### Search

- In regex mode, the replace field understands `\n` for a line break, `\t` for a tab and `\\` for a backslash. Any other escape is written as typed.

#### Editor

- A file with nothing in it opens in the editor, ready to type, from the command line, the sidebar or the open dialog. A file with only spaces and empty lines counts as empty.
- `Shift+Enter` in a markdown file adds a line break: Oryx types the two spaces markdown needs at the end of the line, and the next line stays in the same paragraph, list item or quote. In a code or text file it is the same as `Enter`.
- With nothing selected, `Ctrl+C` copies the line the caret is on and `Ctrl+X` cuts it. `Ctrl+V` puts it back as a whole line above the current one.
- `Ctrl+K` with nothing selected takes the word under the caret as the link's text and puts the caret in the parentheses, ready for the address. On a web address, the address becomes the link's target.
- `Alt+Right` goes forward again after `Alt+Left`. The back and forward buttons of a mouse do the same. A search hit taken with `Enter`, a jump to a line, to the top or to the bottom, and another file you opened are places you can come back to. `Alt+Left` reopens the file you came from where you left it, and asks first if it has unsaved edits. Both keys work in the editor too, and move the caret.
- In a markdown file, `Ctrl+V` pastes a picture from the clipboard. Oryx saves it as a PNG in an `images` folder next to the file and adds the link, with the caret in the brackets for the description.
- An image file dropped on a markdown file you are editing is added to it. A picture already under the file's folder is linked where it is, any other is copied into `images`. A note from `Ctrl+M` has to be saved first.
- A pasted or dropped picture larger than 2560 pixels on its longer side is made smaller: a 4800 by 3200 photo added half a second and 130 MB each time its file opened, at 2560 it adds 70 ms and 19 MB. `Ctrl+Shift+V` pastes the picture as it is. Pictures already in the file's folder are not resized, GIF, WebP and SVG files neither. The file you drop is not changed, Oryx resizes its own copy.
- A double click on a word lights up every other place the same word appears: whole words, same case, on a markdown page, in a code file, in a book and in the editor. The marks go away with the selection.
- A click with `Shift` held extends the selection to the click. In the editor, with nothing selected, it selects from the caret to the click.

#### Files

- When the open file is deleted or moved outside Oryx, a notice says so, the title shows the unsaved dot, and Oryx asks before closing. `Ctrl+S` writes the text back where the file was; if its folder is gone too, Save As writes it elsewhere. A file missing for an instant while another program saves it is not taken for a deleted one.
- A diff shows its added lines in green, its removed lines in red and its `@@` lines in blue, in every theme, from a `.diff` or `.patch` file or from a pipe. The colors are the theme's own, those of its tip, caution and note alerts.
- Text can be piped into Oryx: `git diff | oryx`. Oryx reads it before the window opens and guesses what it is, a diff, JSON or a script. `--as md` shows it as another kind, and `oryx -` reads standard input. The text is kept in a temporary file that goes when Oryx quits; `Ctrl+S` asks where to save it, starting in the folder you ran the command from.

#### Sidebar

- A middle click on a file opens it in a second Oryx window, a step down and right of the first (on Wayland your desktop places it); the file you were editing stays as it is. `Ctrl+Enter` on the highlighted row does the same from the keyboard. The two windows share the settings and the book positions, and each saves only what it changed.
- Files and folders whose name starts with a dot are hidden. `Ctrl+Shift+H` shows them, dimmed, and hides them again; the choice is remembered. The file you are reading keeps its row either way.
- The sidebar follows changes on disk: a file added, removed or renamed in a folder it shows appears or goes the next time you come back to the window or touch it.

#### Quick reference

- The F1 page is now the quick reference, in two parts: the shortcuts, then the whole markdown syntax reference, every construct Oryx understands shown as written and as rendered. Two links at the top jump to either part, and the outline in the sidebar lists both.

#### Embedded HTML

- `<p align="right">` and `<div align="right">` put their content against the right edge. `align="left"` works too, and when aligned blocks are nested the innermost one decides.

#### Files without an extension

- A file without an extension gets syntax colors when it says what it is: a script by its shebang, a file with an editor modeline, a dotfile by its name (`.bashrc`, `.zshrc`, `.profile`, `.gitconfig`, `Gemfile`, `PKGBUILD` and the like), or a diff, a JSON, an XML or an INI file by the shape of its first lines. Anything else opens plain.

#### Source view

- Heading lines are bold in the source view, as they are on the page. A code font without a bold face shows no difference.

#### Line numbers

- A new `line numbers` row in the settings (`Ctrl+,`) numbers the lines in the left margin: in code files, in text files, and in a markdown file while you edit it. Off by default, remembered once set. The rendered markdown page and the PDF stay without numbers.
- The numbers sit in the margin the page already has, so turning them on moves nothing. When the digits need more room (a very long file, a narrow window, a big zoom), the text steps right by what is missing.
- A line that wraps gets its number on its first row only. Copy never takes the numbers. In the editor, the number of the caret's line reads brighter.

#### Go to line

- `Ctrl+G` opens a small field where the search bar stands. Type a line number and press Enter, and Oryx goes there. `412:10` lands on the tenth character of line 412. A number past the end goes to the last line.
- In the editor the caret lands on the line. While reading a code or text file the line comes to the top, and on a rendered markdown page the block that holds that line of the source does. `Alt+Left` goes back to where you were. Books and comics have no lines, so the key does nothing there.
- From a terminal, `oryx main.rs:412` and `oryx main.rs:412:10` open the file at that place, the form compilers print. A file really named `notes:412` still opens as typed.

#### Word count

- A new `word count` row in the settings (`Ctrl+,`) shows the size of the file in the bottom right corner: words, characters, lines and reading time. Off by default, remembered once set. Select some text and the line shows the figures of the selection.
- The count reads the text as the page shows it: markdown marks, link addresses, HTML tags and frontmatter are not counted. A line ending with two spaces counts as a line, a plain line break inside a paragraph does not. Reading time is 240 words a minute.
- A code file shows its lines and characters only. Books and comics show nothing.

#### Autosave

- Two new rows in the settings (`Ctrl+,`) save your file without `Ctrl+S`. `save on focus loss` writes it when you switch to another window. `save after a pause` writes it once you have stopped typing for the time you choose: 5, 15 or 30 seconds, or 1, 5, 10 or 15 minutes. Both are off by default. A burst of typing costs one write, and nothing runs while Oryx is idle.
- An automatic save never writes over someone else's change: if the file changed or was deleted on disk, Oryx says so and waits for your `Ctrl+S`. It also waits while the unsaved-changes question is on screen. A note (`Ctrl+M`) has no file yet and is not autosaved.
- The caret is hidden while the Oryx window is in the background.

#### Notes

- A note (`Ctrl+M`) survives a crash, a power cut or a lost display. Oryx copies the note's text to a file of its own five seconds after you stop typing, and when the focus leaves the window. The copy is not a save: the unsaved dot stays, and Oryx still asks before closing.
- If Oryx ended without asking, the next launch says "A note from your last session was not saved." `R` or Enter recovers the note, `D` discards it, and Escape leaves it for the next launch. The note comes back unsaved, in this window, or in a second window when Oryx was started on a file.

#### Welcome page

- The welcome page shows a tip, a different one at each launch, from a list that covers every feature of Oryx. The tips take turns between the areas (files, moving around, search, editing, themes, export, markdown, books), so ten launches show ten different kinds of things. A "more tips" link under it shows another. Nothing to close and nothing to switch off: the tips only live on the page you see when no file is open.

### Fixes

- Searching for a space or a tab now highlights the ones at the end of a line, and the ones where a long line wraps. Before, the count was right but those matches had no box, and Enter could land on one you could not see. A selection that reaches the end of a line covers its trailing spaces too.
- `\n` in regex mode finds the line breaks. Before, a pattern made only of line breaks found nothing. Replace can join lines, collapse blank lines or add a line after every line, and copy gives the line breaks. A match never crosses from one block to the next, and the file's final line break stays.
- Empty lines added at the end of a file get their rows. Before, each Enter at the end of a long file added a line the page did not show: the caret left the screen and the wheel could not reach it.
- The page no longer moves by a pixel on every keystroke at a fractional display scale, such as 125%.
- `Ctrl+B`, `Ctrl+I` and `` Ctrl+` `` work the way you expect: press the key, type, press it again and go on in plain text. Before, the second press wrapped the last word once more and left stray stars. At the end of a word, the key leaves the caret after the closing mark, so you can write on.
- The same keys remove the marks around the caret whatever the number of words between them. Before, only a single marked word could be unmarked. Pressed twice on an empty spot, the key removes the empty pair it just made.
- `Tab` in a new markdown file inserts four spaces, which nest under every list marker. Before, it inserted a tab character. A file that already indents with tabs or with two spaces keeps its own, and a new code or text file still gets a tab.
- In a markdown file, `Tab` no longer turns a list into a code block. A list item nests under the item above it, one level at a time. With no item above, or when the item is already nested, `Tab` leaves the list alone. Before, `Tab` on a first-level list put four spaces in front of it, which markdown reads as code.
- `Tab` on a list item inside a quote (`> - item`) nests it inside the quote, and `Shift+Tab` brings it back. Before, the spaces went in front of the `>`, and markdown read the line as plain text.
- In a markdown file, `Ctrl+E` and `Escape` keep the line you are on at the same height on the screen. Before, the line jumped to the top of the window each time.
- The caret comes back on the line it left when that line is an image, an empty line or a rule. Before, it came back on the paragraph above, or on the first line of the file when a tall image filled the window.
- `Up` on the first line goes to the start of the line, and `Down` on the last line to its end. With Shift, the selection follows.
- Oryx no longer keeps a processor core busy after you leave the editor with Escape.
- When the connection to the display is lost (a compositor that crashes or drops the window), Oryx closes in order, saves its settings and says why in the terminal. Before, it could crash on its way out, mostly while a big file was loading.
- Fixed a rare crash while a big markdown file with many code blocks was still loading: syntax colors arriving at the wrong moment could leave the page's bookkeeping out of step. Seen on an 8 MB file opened straight at a far line.
- A code or text file you come back to in the same session reopens at the line you were reading. Before, only markdown files and books kept their place.
- The sidebar opened from a new note (`Ctrl+M`) shows the folder you were in. Before, it showed Oryx's own folder for notes.
- `Ctrl+Shift+S` works on a file you are only reading. Before, it did nothing until the file had been in the editor once.
- The file Oryx was started with reloads when it changes on disk, like any file opened later.
- A file whose lines end in CR CR LF (an old Mac file converted to Windows, or converted twice) no longer breaks the editor. Before, each typed letter pushed its line down on screen, and Oryx soon crashed. Every return before a line break is kept and written back on save, so an untouched line keeps its bytes; a new line takes the file's usual ending. Pasted text is cleaned the same way. A classic Mac OS file, whose lines end in CR alone, reads as lines too and saves with CR again. Before, it opened as one long line.
- A binary file whose first zero byte sits past the 8 KB Oryx reads is refused, like any other binary. Before, a cartridge ROM opened as text, and its random characters made Oryx load every font on the system, 700 MB and ten seconds of freeze. A file is binary when its first 8 KB holds a zero byte or is more than 30% unreadable bytes. A text file in an encoding older than UTF-8 (Cyrillic in CP1251, Japanese in Shift-JIS) is refused the same way, with the message "is not UTF-8 text".
- Control characters in a text file (the escape codes of a log, for example) no longer freeze Oryx. They show as spaces. Before, each one made Oryx search every font on the system for a glyph: a log of 190 KB took a minute and a half to open, it takes a fifth of a second now.
- A fresh install opens with the sidebar showing, at your home folder. Before, the sidebar was closed, and once opened it showed the folder Oryx was started from, which on Windows can be a system folder. Close it once and it stays closed. The open and export dialogs start in your home folder the same way.
- A folder Oryx cannot read shows one row saying so, under the `..` row, so you can climb back out. Before, the panel went blank.
- The welcome page says that the sidebar key shows and hides the panel.
- The folder you are in reads in the accent color in the sidebar, its name and its icon: the folder of the open file, or, with no file open, the folder you last clicked. Before, the folder you had opened and the folder under the mouse looked the same.
- In a right-to-left page, `align="center"` and `align="left"` move the text. Before, it stayed on the right.
- The checkboxes of task lists are a little bigger, and the check mark fills the box. The click target and the PDF follow.
- In the font list of the settings, a click on the hint under the list no longer picks a font you could not see.

## v1.1.1

A small release for the first two reports from a user, issues #1 and #2 on GitHub. The details, by area:

### Page breaks

- You can now force a page break in the PDF. Write `<div style="page-break-after: always"></div>` or `\newpage` on a line of its own. The PDF starts a new page there, and the window shows a dashed line. `\pagebreak` and `\clearpage` work too. SYNTAX.md lists every spelling.
- Two breaks in a row make one new page. A break at the end of the file adds no empty page.

### Source view

- The `---` of a rule and the `>` of a quote were nearly invisible in most themes when a file showed as source (Ctrl+E). They took the color of the thin line they draw. They now use the punctuation color, and quoted text shows in the normal text color.
- Bold and italic inside a quote keep their colors in the source view.
- Code comments were too faint to read in eleven themes. Each comment color moved one step darker or lighter, and a test now keeps every theme readable.

## v1.1.0

This release is about the editor and the markdown it reads. Added the small tricks a real editor has, a quick note on Ctrl+M, search fields that behave like text boxes, a new look for the sidebar and for the unsaved changes dialog, and the extended markdown syntax, so every construct of the Markdown Guide's test file renders. The details, by area:

### Lists and headings

- Enter on an empty list item ends the list. Before, if you had clicked into the item, Oryx left the dash behind.
- Alt+- turns the selected lines into a bullet list, Alt+1 into a numbered list and Alt+X into a task list. Press the same key again to remove the markers. One Ctrl+Z undoes it.
- Alt+. quotes the selected lines. Press it again to remove the quote.
- Ctrl+1 to Ctrl+6 set the heading level of the line. The same level again makes it plain text.
- Ctrl+L ticks or unticks the task box of the line, or of every selected line that has one.
- Enter in the middle of a numbered list renumbers the items below. One Ctrl+Z takes back the new line and the numbers together.

### Text and lines

- Type `(`, `[`, `{`, `"` or `'` with text selected and Oryx wraps the text instead of replacing it. In a markdown file `*`, `_` and a backtick wrap too, so two stars make bold.
- Ctrl+B, Ctrl+I and Ctrl+` make the selection or the word under the caret bold, italic or code. The same key again removes it.
- Ctrl+K turns the selection into a link and puts the caret where the address goes. Pasting an address over selected text makes a link too.
- Alt+Up and Alt+Down move the line, or the selected lines, up and down.
- Ctrl+Shift+D duplicates the line or the selected lines. Ctrl+Shift+K deletes them.
- Ctrl+/ comments or uncomments the line or the selected lines, in the language's own way. In a markdown file it uses an HTML comment.
- A keyboard selection can now reach the empty line at the end of the file.
- Syntax colors arrive while text is selected, and right after a shortcut moves or rewrites lines. Before, those lines stayed uncolored until you cleared the selection.
- Shortcuts with a digit, a period or a dash also answer the key at that position on the keyboard. They now work on a French keyboard, where those keys need Shift.

### The caret

- A click on an empty line puts the caret there. Before, only the arrow keys could reach an empty line.
- A click or an arrow key past the end of a line lands after the trailing spaces, where End goes.
- The caret shows on an empty file, and a click on an empty page places it.

### Search

- The search and replace fields behave like text boxes. Ctrl+Left and Ctrl+Right jump words, Shift selects them, Ctrl+Backspace and Ctrl+Delete remove a word, Ctrl+Y redoes. A click places the caret, a double click selects the word, a triple click selects everything, and a drag selects.
- While a search field has the keyboard, the document's editing keys stay quiet. Before, Ctrl+B or Tab typed into the search field changed the document. The fields of the theme editor and the theme browser gain the same keys.
- The search bar's text sits level with the caret and the regex button.

### Notes and saving

- Ctrl+M opens an empty markdown note in the editor, with no dialog. You choose the name, the place and the type at the first Ctrl+S. Until then the note is unsaved work, and Oryx asks the usual question before closing or opening another file.
- After Save As, the sidebar shows the folder you saved in.

### Markdown

- Heading IDs: `## Title {#custom-id}` gives the heading that anchor, and the braces stay out of the title.
- Definition lists: a term line followed by `: definition` lines renders like the HTML form, the term bold and the definitions indented.
- `H~2~O` and `x^2^` render as subscript and superscript. A tilde between spaces still strikes through, as on GitHub.
- `==text==` and `::text::` highlight the text.
- Abbreviations: a `*[HTML]: Hyper Text Markup Language` line marks every HTML in the document with a dotted underline, and resting the mouse on the word shows the expansion.
- Footnotes are numbered in order of use, as on GitHub, whatever their labels. A footnote of several paragraphs shows its number once, with the other paragraphs indented under it. Before, the label repeated on every paragraph.
- The number at the foot of a footnote links back to where it is cited, in the window and in the PDF.
- The PDF export carries all of the above.
- SYNTAX.md was rewritten. Every construct is shown twice, as written and as rendered.
- Big markdown files take less memory. An 8 MB markdown file settles at 172 MB instead of 201 MB, and its peak while loading is 30 MB lower.

### The look

- Redesigned the sidebar. Folders have a small triangle and an outlined folder icon. Files have an outlined icon by type. Children hang from thin guide lines. The open file takes the theme's accent color, with a bar at its left. The row under the mouse lights up, and the list has a scrollbar, for the files and for the outline. Every theme works as it is.
- Redesigned the unsaved changes dialog in the same spirit. The page dims behind it and the file's name is shown. The three answers are rows with their key beside them: S saves, D discards, Escape keeps editing. The arrows move between the rows and Enter or Space picks the highlighted one. A click on a row picks it, and a click outside keeps editing.
- The sample image in the examples folder shows the current logo.

### Fixes

- The theme browser opens with the current theme in the middle of the list, so you see where you are. The list has a scrollbar now.
- In the theme browser, the last row no longer shows below the panel when you scroll with the wheel.
- On the help page, the Ctrl+` shortcut shows its key. Before, the backtick broke the table cell.
- A bare URL with a tilde in its path, like `https://cs.edu/~name/`, is linked whole. Before, the link stopped at the tilde.

## v1.0.0

Version 1.0.0 is the first one published as packages: a `.deb`, an `.rpm` and an AppImage on the release page, two AUR packages, and the Windows installer, beside the tarball and the zip as before. The details:

- Oryx ships as a `.deb` and an `.rpm` on the release page, beside the tarball, the zip and the MSI.
- An AppImage is on the release page too: one file that runs on any Linux with glibc 2.35 and OpenSSL 3, nothing to install.
- On Arch Linux, Oryx is in the AUR as `oryx-editor` (built from source) and `oryx-editor-bin` (the release binary).
- Oryx is in the Microsoft Store as Oryx Editor, free, for Windows 10 and 11. The Store package cannot claim the file types Windows reserves for itself (`.bat`, `.cmd`, `.js`, `.pl`, `.py`, `.rb`), so under the Store install those files are opened from inside Oryx (Ctrl+O or the sidebar) rather than from the file manager; the MSI keeps every type.
- On Windows, `winget install Steerania.Oryx` installs the MSI.
- Headings and bold words stay in the chosen body font when that font has no bold face (a cursive or display font, for example). Before, they switched to another font, on screen and in the PDF.
- In the editor, a checkbox followed by a word in parentheses (`- [ ] (Ivan) call back`) no longer shows the parentheses in the link color. The old Markdown allowed a space between `[text]` and `(url)`; the page does not, and the editor now agrees with the page for links, references and images.
- The Linux binary is built on Ubuntu 22.04 and runs on Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.4 and newer (glibc 2.35, OpenSSL 3).
- The Windows installer is 9 MB instead of 26: the old one carried the program twice, once as the file and once as the source of its icon.
- On Linux, Oryx registers itself as `com.steerania.Oryx`, so the window icon shows on Wayland desktops; an earlier registration is cleaned up. Under a package, `oryx --register` says the package already did it.
- Themes are also found in the system folders where a package installs them (`/usr/share/oryx/themes` and the other `XDG_DATA_DIRS` entries).
- Clicking a heading in the outline goes to that heading, not to the first one with the same text. Repeated headings get numbered anchors as on GitHub (`#title`, `#title-1`).
- A privacy page, `PRIVACY.md`: no account, no telemetry, what is kept on the machine and where.
- The `F1` page links to the documentation on GitHub, the project's home.
- The welcome page points at the settings (`Ctrl+,`, for the fonts, the sizes and the interface scale) and at the documentation on GitHub. The `F1` page says what `Ctrl+,` opens, what `Left` and `Right` do (move to the sidebar and back), and what `Enter` does in the sidebar.

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

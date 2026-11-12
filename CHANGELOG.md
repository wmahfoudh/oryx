# Changelog

## v1.4.0

### Fixes

- A definition list with a tab before its colon no longer crashes Oryx. Before, Oryx crashed when the definition above it ended with an accented letter or a non-Latin character.
- A list item that holds a link definition, followed by a line of spaces, no longer crashes Oryx.
- A line of spaces or tabs after a link definition is now read as a blank line. Before, it was read as text, and the paragraph after it started with an empty line.
- `Alt+Escape` in the Go to line field, or in the field that asks for a file's path, now closes the field as `Escape` does. Before, it quit Oryx or left the editor.
- On Linux without a desktop portal and without zenity, the Save of the unsaved-changes question on a note or on piped text is now followed by what the question announced, once you have given the path: Oryx quits, opens the file, or starts the new file or note. Before, the text was saved and nothing followed.
- The case key (`Ctrl+U`) now changes the first word of a footnote's text. Before, that word was left as typed.
- Enter held in the Go to line field no longer adds empty lines after it goes to the line. A held key now stops repeating once its first press has closed a field or a dialog, or changed the mode.

## v1.3.0

This is a feature release, about the editor and the math. New in this release: the syntax colors now follow each key when you type, where they came about half a second later. Formulas accept the TeX commands Wikipedia uses, and a new page teaches the math notation and lists the commands. An image alone in its paragraph is centered. The page scrolls when you select with the mouse past the top or the bottom of the window. You can create a file from the command line, and update the AppImage in place. Formulas are now typeset in table cells and in footnotes. Several crashes are fixed: in the PDF export, in formulas and in the editor. On Linux, the file dialogs now work on a system without a desktop environment.

### New features

#### Editor

- When you type in a code file or in the source of a markdown file, the syntax colors now follow each key. Before, they came about half a second after the last key.
- From the command line, `oryx notes.md` now opens `notes.md` as a new file in editing mode if it does not exist. The file is created when you save. Before, Oryx said the file could not be read.

#### Linux

- You can now update the AppImage in place with AppImageUpdate, AppImageLauncher or Gear Lever. Before, you downloaded each new version yourself.

#### Math

- Formulas now accept the TeX commands Wikipedia's math uses: `\tfrac`, `\dfrac`, `\cfrac`, `\tbinom` and `\dbinom`, `\pmod` and `\mod`, `\mathop` and the other class commands, `\boldsymbol`, `\textrm`, `\hbox`, and about a hundred names such as `\sgn`, `\arccot`, `\R`, `\infin` and `\rarr`. `\mathrm dx` without braces works as in TeX. Before, these commands were shown as written.
- Formulas now accept `\big`, `\Big`, `\bigg` and `\Bigg` before a delimiter, with their `l`, `r` and `m` forms, as in `\bigl( x \bigr)`. Before, these words were shown as written.
- Formulas now accept TeX's `\over`, `\atop`, `\choose`, `\brace` and `\brack` between the two halves of a group, as in `{n \choose k}`, and the switches `\bf`, `\rm`, `\it`, `\sf`, `\tt`, `\cal`, `\displaystyle`, `\textstyle`, `\scriptstyle` and `\scriptscriptstyle` over the rest of a group. Before, these words were shown as written.
- Formulas now accept `\overline` and `\underline`, `\overbrace` and `\underbrace` with their limits, `\stackrel`, `\overset` and `\underset`, `\xrightarrow` and the similar arrows with a text above and below, `\not` before a symbol, `\phantom`, `\vphantom` and `\hphantom`, and `\color` and `\textcolor` with xcolor's names or a `#rgb` or `#rrggbb` value. Before, these commands were shown as written.
- A new page, `MATH.md`, has a tutorial of the math notation and a reference of the commands Oryx knows. `F1` opens it from the Math link on the first line of the quick reference. Before, the syntax page had a short math section, and the examples folder a math sample.
- The example file `markdown-challenge.md` now has the math of its Wikipedia articles as Wikipedia writes it, 2,949 formulas. Before, the formulas Oryx could not typeset were rewritten, or left out, when the file was made.

#### Reader

- An image alone in its paragraph is now centered, with or without a link around it. An image with text on its line, a line of badges and an image in a list item are unchanged. Before, the image was at the left.
- `align="left"`, `align="right"` or `align="center"` on an `<img>` tag alone in its paragraph now aligns the image. Before, the attribute was ignored.

#### Selection

- When you select with the mouse and drag above or below the window, the page now scrolls and the selection grows while you hold the button. The page scrolls faster when the pointer is farther from the window. Before, the page did not move.

### Fixes

- In the markdown editor, a click on a heading in the Outline tab now lands on that heading after you edited the text above it. Before, the cursor landed where the heading was before the edit, and the next key could crash Oryx.
- When you leave the markdown editor, the page now shows the text as you left it. Before, after an undo followed by a new edit, the page could show an older text, and a checkbox clicked on that page wrote into the wrong place of the file.
- When you answer Discard to the save question and then cancel the next step, for example the new-file dialog, the text now comes back as it was last saved. Before, the discarded edits stayed on screen, and the next save could corrupt a file with Windows line endings.
- On the help page, a click on a heading in the Outline tab now goes to that heading while a markdown file is being edited behind the page. Before, nothing happened.
- `Ctrl+U` no longer changes the case of a display formula written on separate lines, or of the inline code, link addresses and tags between two prices such as `$5` and `$6`.
- `Ctrl+U` now reads formulas the way the page does. Text between two dollar signs that the page shows as text changes case. A formula with another formula inside braces is unaffected. A dollar sign above a quote, a list or a heading is not paired with a dollar sign below it. Before, the key and the page disagreed in these cases.
- `Ctrl+U` now changes text between angle brackets that is not an HTML tag, such as `<Hello, World!>`. Before, this text was unchanged.
- `Ctrl+U` no longer changes the case of the code in a code block. Before, the code changed case in an indented block, and in a fenced block that had a blank line.
- Exporting to PDF a page with a long code block inside a quote no longer crashes Oryx.
- After a drag of the scrollbar, the other occurrences of the selected word are now highlighted on the rows around the view. Before, only the rows on screen at the end of the drag were.
- On a Mac, `Option+-`, `Option+1`, `Option+X` and `Option+.` now type their characters in a code or text file. Before, they typed nothing there.
- A settings file that is a symbolic link, as a dotfiles folder makes, is now written through the link. Before, the first change of a setting replaced the link with a plain file.
- Opening a pipe or a device from the Open dialog now shows a notice. Before, the window stopped responding.
- A selection dragged from the end of a paragraph into the next now highlights the other occurrences of its word. Before, it did not.
- The word keys, Ctrl+Backspace, Ctrl+Delete and a double click now take a joined emoji such as 👩‍💻 whole, on the page and in the search field. Before, Ctrl+Backspace deleted one part of it and left an invisible joiner behind.
- When the search bar or a dialog is open, the cursor in the text is now hidden. Before, it kept blinking, for example after Replace All. What you typed then went into the search field.
- When you type in the search bar, Go to line, or the name or color field of a theme, the cursor now blinks, as it does in the text. Before, it did not blink.
- When you search a page with a regular expression such as `[\s\S]+`, each paragraph is now matched separately. Before, only the last paragraph matched. On a long page, Oryx froze.
- When you use Replace All in a file with both Windows and Unix line endings, the lines between the matches now keep their own ending. Before, they took the file's usual ending, and the save notice counted them as changed.
- In a markdown file with a stray carriage return, the editor no longer shows an extra blank row. Before, it could appear after you deleted the text between the carriage return and the end of its line, then edited the next line.
- When you indent lines, or make them a list, a quote, a heading, a task list or a comment, in a file with both Windows and Unix line endings, each line now keeps its own ending. Before, the lines took the file's usual ending.
- When you scroll with the search bar open, the matches that come into view are now highlighted at once, down to the end of the page. Before, they were highlighted late, and the last paragraphs could stay plain.
- When a search match is taller than the screen, such as `[\s\S]+` in the editor, the part on screen is now highlighted wherever you scroll. Before, the match lost its highlight when you scrolled down.
- When you open a comment in a big code file and type above it while the lines below are still being colored, the lines below are now colored to the end of the file. Before, the lines the coloring had not reached kept their old colors. The same happened with two keys typed at the same time on different lines.
- A key typed on a line far below the colored lines of a big code file, right after the file opened or after a zoom, no longer crashes Oryx.
- In a formula, `~` is now a space, as in TeX. Before, it drew a tilde.
- In a formula, a superscript or subscript after a spacing command such as `\,` is now shown. Before, it was dropped.
- A formula where the argument of a command is an empty macro, such as `\frac1\e` after `\newcommand{\e}{}`, no longer crashes Oryx.
- In a formula, the text of `\text` or `\mathrm` written inside a macro, such as `\newcommand{\st}{\text{ s.t. }}`, is now shown. Before, it was blank.
- In a formula, `\left<` and `\right>` now draw angle brackets, as in TeX. Before, they drew the less-than and greater-than signs.
- In a formula, a bar, a double bar or an arrow stretched with `\left` and `\right` now fits what it encloses. Before, it could be taller than needed.
- In a formula, a plus or minus sign right before a closing bracket, a relation or a comma, as in `(a+)`, now has no space around it, as in TeX. Before, it had spaces.
- In a formula, the limits of an integral are now at the top and at the bottom of the sign, as in TeX. Before, both were near the middle.
- Typing in a plain text file no longer runs the syntax coloring on each key. Before, each key could cost a few milliseconds for nothing.
- A key on a long markdown line with several code spans or links no longer freezes the editor while that line is parsed. The line is now colored in the background. Before, each key on such a line could cost 50 ms.
- When the colors below a key are recomputed right after the cursor moved to a far line, they are now shown at once. Before, they could come up to half a second later.
- After F1 and back while a big file is still being colored, the lines on screen are now colored at once. Before, they stayed plain until the coloring reached them.
- A named pipe in a folder the sidebar shows no longer freezes Oryx. Before, the window froze until a program wrote to the pipe.
- The sidebar no longer lists a link to a file that is gone. Before, a click on it did nothing.
- A formula in a table cell now shows a command written as a superscript or a subscript, such as `e^\pi`, as its symbol. Before, the cell showed `e` with a raised `\p`, then `i`. The same is fixed in the copied text and in the search.
- A formula in a footnote is now typeset. Before, the footnote showed it as its TeX text.
- A code block, a quote, a heading or a table written right under the text of a list item, with no blank line between them, is now shown after that text. Before, the code block was shown above the item, and the quote, the heading or the table was joined to the item's text.
- A command inside `\mathrm{...}` or `\operatorname{...}` is now typeset, as in `\mathrm{kg\,m^{-1}}` and `\operatorname{log\Gamma}`. Before, the command was shown as written.
- `\liminf`, `\limsup`, `\injlim` and `\projlim` now have the thin space between their two words. Before, they were written as one word.
- `\text{...}` written over two lines now reads as one line of text, and `\%`, `\$`, `\&`, `\_` and `\#` inside it are the characters. Before, the line break and the backslashes were shown.
- A formula with thousands of nested braces, superscripts or subscripts no longer crashes Oryx. Past a limit, the inner part is shown as written.
- A zipped FB2 book whose inner file name is in an old Windows encoding, such as a Russian name in CP866, now opens. Before, it was refused as not readable.
- A comic zipped on a Mac now shows only its pages. Before, the hidden `__MACOSX` entries showed as blank pages at the start.
- A damaged book or comic whose header declares an impossible size is now refused as damaged. Before, Oryx could crash on it.
- On a Mac, `Option+R` now types ® in the editor. Before, it typed nothing. The key still switches regular expressions on and off in the search bar.
- On a Mac, `Option+-`, `Option+1`, `Option+X` and `Option+.` now type their characters in the search bar, such as an en dash for `Option+-`. Before, they typed nothing there.
- Duplicating or editing a bundled theme now works with every install. The copy is saved in your themes folder: `~/.local/share/oryx/themes` on Linux, `%APPDATA%\oryx\themes` on Windows, `~/Library/Application Support/oryx/themes` on a Mac. Before, with a deb, an rpm, the AppImage, the Mac app, the MSI or the Store, the copy failed with no message.
- When a theme cannot be saved, a notice now says so with the reason. Before, nothing showed.
- `install.sh` and `make install` now keep your custom themes and replace the bundled ones. `install.sh --uninstall` keeps your custom themes too and tells you where they are. Before, an install deleted the themes folder, your custom themes included, and the uninstall deleted the whole data folder.
- On Linux, Oryx now starts from the launcher and from "Open with" when its program file is in a folder with a space or a special character in its name, such as an AppImage under `My Apps`. Before, the launcher could not start it.
- A file Oryx cannot read, such as a damaged book, now opens a window that says why when you double click it. Before, nothing appeared, and the reason was printed in a terminal you did not see.
- A file you can write in a folder you cannot write to is now saved. Before, the save failed with "Permission denied". On Linux and Mac, a file with a second hard link keeps its link, and a file owned by someone else keeps its owner. Before, a save broke the link and made you the owner.
- The zip library, used for EPUB books, CBZ comics and zipped FB2 books, was updated to version 9. Three security advisories were published on the old version in October 2026. They are about extracting an archive to disk. Oryx does not do that. No release was affected.
- A formula inside a table cell is now typeset like the formulas of a paragraph, and the row grows to its height. Before, the cell showed the formula's text.
- A `$$` formula inside a table cell now stays in its cell, in display style. Before, it was shown as a block above the table, and its cell lost it.
- An inline formula wider than its column or its cell now shrinks to fit, down to half its size, as a display formula does. Before, it ran past the right edge. The picture of such a formula in a copy is whole too. Before, it was cut at the right.
- On Linux without a desktop portal and without zenity, such as a bare window manager on Wayland, the file dialogs (open, save as, new file, export) now ask for the path in a field at the top of the window. Enter opens, saves, creates or exports to that path, and Escape cancels. Before, nothing happened.
- Two footnote references next to each other, such as `[^1][^2]`, are now separated by a comma. Before, they looked like one number.
- Exporting to PDF a page with an image taller than the PDF page no longer crashes Oryx.
- In the PDF, a centered image taller than the page is now centered. Before, it was left of center.

## v1.2.2

This is a maintenance release: mostly fixes, and a few additions. Keyboard shortcuts now work on Cyrillic, Greek and other non-Latin layouts, and on a Mac the Option key types its characters as usual. Right-to-left text works better in the editor, in the search bar and in the outline of a book. New in this release: a key to change the case of a selection, a highlight of the other occurrences of a selected phrase, and the alignment of table columns. Syntax colors no longer disappear after an undo or a quick edit. The search bar is fast again with a very long query: a few milliseconds per key instead of seconds. The help page no longer uses a full processor core, and the window is no longer transparent on KDE Plasma 6.8.

### New features

#### Books

- Images, including the cover, are now center-aligned, unless the book's stylesheet places them. Before, they were left-aligned.

#### Editor

- `Ctrl+U` changes the case of the selected text, or of the word under the cursor: lower case, then Capitalized Words, then UPPER CASE, one press after the other. The text stays selected between presses. In a markdown file, link addresses, inline code, HTML tags and formulas are unaffected.

#### Examples

- The examples folder now has `markdown-challenge.md`, an 8 MB markdown file of real text: Wikipedia articles with formulas and tables, and public-domain books. Open it in Oryx, then in your other markdown editor.

#### Selection

- When you select several words on one line, with the mouse or the keyboard, the other occurrences of the same text are now highlighted, as for a double-clicked word. The highlight appears when you release the mouse button, or after a short pause in typing.

#### Themes

- A new theme, `on-the-field`: a light page in green tones.

### Fixes

- On Linux, the AppImage now includes the X11 keyboard libraries, and the deb and rpm packages declare them. Before, on a system without `libxkbcommon-x11`, Oryx crashed at start with an error trace. Now it tells you which package to install.
- Shortcuts now work on non-Latin keyboard layouts, such as Cyrillic, Greek or Arabic, and on layouts with accented letters on letter keys, such as BÉPO: `Ctrl` with the key at the place of C is `Ctrl+C`. The same goes for `Ctrl+,` and, on a Mac, for `Cmd+[` and `Cmd+]`. Before, letter shortcuts did not work on these layouts, in the dialogs and the search bar either (issue #8).
- On a Mac, a character typed with Option is now typed as a character: `Option+Shift+-` gives an em dash, and on a French Mac `Option` with the `)` key gives `}`. Before, these keys created a bullet list.
- On a Mac, the list shortcuts with Option now follow the key printed on the keycap: `Option+-` makes a bullet list on a French or German Mac too, where the minus key is in another place.
- `Ctrl+Alt` with a character key no longer triggers a shortcut: on Linux, with the left `Ctrl+Alt` on Windows, and with `Cmd+Option` on a Mac. Before, on a German Windows layout, `Ctrl+Alt+2` set a heading and `Ctrl+Alt+0` reset the zoom, and on Linux `Ctrl+Alt+S` saved the file.
- A double click on a Persian, Kurdish, Urdu or Indic word now selects the whole word. The word keys (`Ctrl+Left`, `Ctrl+Backspace`), the case key and the occurrence highlight also take the whole word. Before, the invisible joiner inside these words split the word in two.
- In a book, a paragraph left-aligned by its own style inside a centered block now stays left-aligned. Before, the block's centering was applied to it.
- In a formula inside a table cell, and in copied text, an escaped character (`\%`, `\#`, `\_`) now shows the character, as in a paragraph. Before, the backslash was shown, and `x\_1` was read as a subscript.
- The help page no longer keeps a processor core busy once its syntax section is on screen. Before, the coloring of its code blocks was restarted again and again until the page was closed (issue #9).
- On a big file, jumping to the end while the file is still being laid out no longer confuses the page layout. Before, the view could show the wrong geometry until it moved again.
- In the Outline tab, a long right-to-left title now shows its beginning and is cut at its end with an ellipsis. Before, the edge of the sidebar cut its beginning, and several chapters looked the same.
- In the Outline tab, a chapter is now highlighted as soon as its heading is in the top half of the window. At the start of a book it is the first chapter in view, and at the end of the book the last one. Before, the previous chapter stayed highlighted while the next heading was near the top, and no chapter was highlighted on the cover of a book.
- An image without a picture now sizes its placeholder to a right-to-left description. Before, the box was too narrow and the text overlapped the words before it.
- In the search bar and in the fields of the theme dialogs, the caret, the selection and clicks are now at the right place in right-to-left text. Before, they were mirrored.
- A line or paragraph separator pasted into the search bar or a field is now dropped, like other control characters. Before, it crashed Oryx.
- In a formula, `\%` now shows a percent sign, and `\#`, `\&`, `\$` and `\_` show their character, as in KaTeX. Before, they were shown as written, in the color of an unknown command. A bare `%` still starts a comment, as in TeX.
- The words that fit after an inline formula now stay on the same line, and the next line starts at the margin. Before, all the text after a formula moved to the next line, which started with a space. With justification on, a line that continues after a formula now ends at the margin.
- With justification on, Latin words at the end of a right-to-left line are now drawn correctly. Before, the first word was drawn over the others.
- In the editor, on a right-to-left line that contains a number, a Latin word or markup, `Home` and `End` now go to the ends of the line, and a click beside the line goes to the nearest end. Before, only the last words of the line counted: `Home` did not move or landed on them, and a click beside the line landed there too.
- After an editing key on a mouse selection, such as `Ctrl+B` or `Ctrl+U`, moving the mouse no longer extends the selection. Before, Oryx behaved as if the mouse button were still held, and the file was not laid out again until the selection was cleared.
- On KDE Plasma 6.8, the window is no longer transparent. KWin 6.8 applies the opacity channel of every window, and Oryx left it empty, which means fully transparent. Oryx now marks every pixel as opaque.
- Table columns can now be aligned, each on its own, with the marks under the header: `:---:` for center, `---:` for right. Before, the marks were ignored. The alignment is kept when the table is copied as HTML (issue #11).
- When the page is scrolled into a tall image and you press `Ctrl+E`, the text under the image now stays where it was on the screen. Before, it jumped up.
- A file that contains a next line character (U+0085) or another control character of the C1 set now opens, with a blank in its place. So does a page whose image description contains one. Before, Oryx crashed when the character stood between two scripts, and an escape character in a description froze the window.
- In the editor, in right-to-left text, the caret is now where the next letter will go: at the right end of a word before its first letter, at the left end after its last letter. Before, it was at the opposite end, one word away from what you typed.
- The search bar is now fast with a very long query. Before, each key took seconds with a query of a few thousand characters.
- In the search bar and the fields, a query longer than the field now stays in place while you move or click inside the visible part, and a letter typed in the middle stays visible. Before, the visible part jumped to the caret after each click or arrow key, and a typed letter disappeared from view.
- A code file now keeps its syntax colors to the end after an undo or a redo in reading mode, and after a key typed while a big file was still being colored. Before, the lines after the first chunk stayed plain until the file was reopened, in the PDF too.
- In the editor, the syntax colors of a code file now stay correct after several quick edits in different places, and after a key typed inside a long comment followed by a scroll. Before, the lines between the edits could stay plain, and lines scrolled into view could show code colors inside a comment, until the file was reopened.
- A key pressed in the editor after the help page no longer recolors the whole file, which took 25 seconds on an 8 MB file. The code blocks of a page, or a code file, are now fully colored after a key pressed just before leaving the editor. Before, the coloring stopped where it was, and the PDF showed the rest plain or with old colors.
- In the editor, the lines below the cursor no longer flash with code colors for a moment after a key typed inside a long comment or string. Before, a quick coloring landed there first, and the right colors came a frame or two later.
- On Wayland, closing Oryx while a big file is still loading no longer prints `Io error: Cannot allocate memory (os error 12)` in the terminal. Before, the part of Oryx that receives dropped files was still reading from the display after the window had closed it.
- The Outline tab is now fast to hover with long chapter titles: 12 ms per frame on a book with long Arabic titles, instead of 72. Before, each frame measured a long title once per removed character.

## v1.2.1

This is a maintenance release focused on stability and fixes, including several on very large files. Most fixes are about keeping the reading position stable, whatever action you perform. The active location and line now stay stable when you zoom, resize the window, open the sidebar, change a font, or switch between reading and editing. Display bugs were also addressed, and Oryx now performs better on large files: a theme change is immediate, and jumps to a line in the editor are a little faster. On Windows, the sidebar issue where it could not reach the other drives was fixed.

### New features

#### Themes

- A new theme, `black-and-white`, made for printing: a white page, black text and grays.

#### Line numbers

- In the editor, the number of the current line is shown in a small box, so it is easier to see.

#### macOS

- Oryx can be installed on a Mac with Homebrew: `brew install --cask wmahfoudh/tap/oryx`.

### Fixes

- In a book, the outline now highlights the chapter you are reading. Before, in some books, the wrong entry was highlighted.
- `Ctrl+G` now shows the line in the middle of the window. Before, the line was at the very top.
- `Alt+Left` and `Alt+Right` now bring you back to the same view you left. Before, the line went to the top of the window.
- `Ctrl+E` in the middle of a table or of an indented code block now opens the editor on the lines you were reading. Before, the editor opened at the start of the table or of the block.
- After `Ctrl+G` while reading, `Ctrl+E` now puts the caret on the line you jumped to. Before, the caret was on the first line in view.
- After `Alt+Left` or `Alt+Right` while reading, `Ctrl+E` now puts the caret back where it was at that place. Before, the caret could be on another line.
- `Alt+Left` now goes back to the file you just left, even when a save failed. Before, in that case, it could stay where it was.
- A jump to an empty line after a long table or a long paragraph now shows the end of that table or paragraph, and so does leaving the editor from such a line. Before, Oryx showed its beginning.
- In a very large markdown file, `Ctrl+E` far down the page now opens the editor on the lines you were reading. Before, the view could end thousands of lines below the caret.
- On Windows, the sidebar can now reach your other drives: `..` at the top of a drive shows the list of drives. Before, the sidebar could not leave the drive it started on.
- On Windows, the open and save dialogs now start in the folder of your file when it is on a network drive. Before, they started in Documents.
- On Linux, the AppImage now starts under firejail. Before, it stopped with "Permission denied".
- The theme editor now saves your changes to `oryx-hero` in a copy, as for the other themes that ship with Oryx. Before, it changed the shipped theme itself.
- The theme browser no longer deletes or renames a theme that ships with Oryx. Duplicate it to get a custom theme that you can change. Before, two clicks on the cross deleted the theme until the next install.
- The sidebar now keeps its width when you open a file from another folder. Before, it went back to its default width.
- A font or a size changed in the settings while the editor or the help page is open now shows on the page when you return to it. Before, the page kept the old one until a reload.
- `Ctrl+D` in the editor now turns the page when you return to it. Before, the page kept its direction until a reload.
- The help page now has its own reading direction. Before, it took the direction of your file, and `Ctrl+D` on the help page changed the direction of your file.
- `Ctrl+Plus` and `Ctrl+Minus` far down a large code or text file now show the text again after a moment. Before, the page stayed blank.
- `Ctrl+Plus` and `Ctrl+Minus` now keep your place: the line at the top of the page stays at the top, and in the editor the line of the caret stays where it is. Before, far down a long file, the view moved to other lines.
- The page also keeps your place when the window or the sidebar changes its width, and when you change a font or a size in the settings. Before, far down a long file, the view moved to other lines.
- Pictures that load from the internet no longer move the text you are reading. Before, when a picture above arrived, the page showed other lines.
- At the end of a long file, the page now shows its text after you close the sidebar or make the window wider, and after the file gets shorter on disk. Before, the page could stay blank.
- A markdown page that has footnotes or a closed section now reopens where you were reading when you come back to it in the same session. Before, it could reopen near the top.
- Changing the theme now keeps the page where you were reading. Before, the view could move.
- In the editor, `Ctrl+End` and `Ctrl+Home` now show the caret in a long file. Before, the view could stop in the middle of the file, and the text you typed next was out of view.
- In the editor, a jump to a far line of a very large file is faster.

## v1.2.0

### New features

#### Copy

- `Ctrl+C` on the page copies the selection with its formatting, so it pastes into an email or a Word document with its headings, lists, tables, quotes, links and code. Pictures and formulas are copied as images. Word processors and desktop mail programs show them, some web mail does not. A terminal or a text editor still gets the plain text. In the editor, `Ctrl+C` copies the text only.

#### macOS

- A macOS build, one app for Apple Silicon and Intel Macs, shipped as a disk image. It is not signed with an Apple developer account, so the first open is blocked. Allow it once in System Settings, Privacy and Security, Open Anyway.
- On a Mac, `Cmd+[` goes back and `Cmd+]` forward, since `Option+Left` and `Option+Right` move by a word there. The quick reference and the tips show these keys on a Mac.

#### Arch Linux

- The release page has a package for Arch Linux, to install with `sudo pacman -U oryx-editor-bin-*.pkg.tar.zst`.

#### Search

- In regex mode, the replace field understands `\n` for a line break, `\t` for a tab and `\\` for a backslash. Any other escape is written as typed.

#### Editor

- An empty file opens in the editor, from the command line, the sidebar or the open dialog. A file with only spaces and empty lines counts as empty.
- `Shift+Enter` in a markdown file adds a line break: Oryx types the two spaces markdown needs at the end of the line, and the next line stays in the same paragraph, list item or quote. In a code or text file it is the same as `Enter`.
- With nothing selected, `Ctrl+C` copies the line the caret is on and `Ctrl+X` cuts it. `Ctrl+V` puts it back as a whole line above the current one.
- `Ctrl+K` with nothing selected takes the word under the caret as the link's text and puts the caret in the parentheses, ready for the address. On a web address, the address becomes the link's target.
- `Alt+Right` goes forward again after `Alt+Left`. The back and forward buttons of a mouse do the same. A search hit taken with `Enter`, a jump to a line, to the top or to the bottom, and another file you opened are places you can come back to. `Alt+Left` reopens the file you came from where you left it, and asks first if it has unsaved edits. Both keys work in the editor too, and move the caret.
- In a markdown file, `Ctrl+V` pastes a picture from the clipboard. Oryx saves it as a PNG in an `images` folder next to the file and adds the link, with the caret in the brackets for the description.
- An image file dropped on a markdown file you are editing is added to it. A picture already under the file's folder is linked where it is, any other is copied into `images`. A note from `Ctrl+M` has to be saved first.
- A pasted or dropped picture larger than 2560 pixels on its longer side is made smaller. A 4800 by 3200 photo added half a second and 130 MB each time its file opened. At 2560 pixels it adds 70 ms and 19 MB. `Ctrl+Shift+V` pastes the picture as it is. Pictures already in the file's folder are not resized. GIF, WebP and SVG files are not resized either. The file you drop is not changed, Oryx resizes its own copy.
- A double click on a word highlights the other occurrences of the same word (whole words, same case), on a markdown page, in a code file, in a book and in the editor. The highlights disappear with the selection.
- A click with `Shift` held extends the selection to the click. In the editor, with nothing selected, it selects from the caret to the click.

#### Files

- When the open file is deleted or moved outside Oryx, a notice says so, the title shows the unsaved dot, and Oryx asks before closing. `Ctrl+S` writes the text back where the file was. If its folder is gone too, Save As writes it elsewhere. A file that is missing for an instant while another program saves it is not reported as deleted.
- A diff shows its added lines in green, its removed lines in red and its `@@` lines in blue, in every theme, from a `.diff` or `.patch` file or from a pipe. The colors come from the theme: those of its tip, caution and note alerts.
- Text can be piped into Oryx: `git diff | oryx`. Oryx reads it before the window opens and guesses what it is, a diff, JSON or a script. `--as md` shows it as another kind, and `oryx -` reads standard input. The text is kept in a temporary file, deleted when Oryx quits. `Ctrl+S` asks where to save it, starting in the folder you ran the command from.

#### Sidebar

- A middle click on a file opens it in a second Oryx window, slightly down and right of the first (on Wayland your desktop places it). The file you were editing is unchanged. `Ctrl+Enter` on the highlighted row does the same from the keyboard. The two windows share the settings and the book positions, and each saves only what it changed.
- Files and folders whose name starts with a dot are hidden. `Ctrl+Shift+H` shows them, dimmed, and hides them again. The choice is remembered. The file you are reading is always listed.
- The sidebar follows changes on disk. A file added, removed or renamed in a folder it shows is updated the next time you come back to the window or use the sidebar.

#### Quick reference

- The F1 page is now the quick reference, in two parts: the shortcuts, then the markdown syntax reference, with each construct shown as written and as rendered. Two links at the top jump to either part, and the outline in the sidebar lists both.

#### Embedded HTML

- `<p align="right">` and `<div align="right">` align their content to the right. `align="left"` works too, and when aligned blocks are nested the innermost one is applied.

#### Files without an extension

- A file without an extension gets syntax colors when Oryx can tell what it is: a script by its shebang, a file with an editor modeline, a dotfile by its name (`.bashrc`, `.zshrc`, `.profile`, `.gitconfig`, `Gemfile`, `PKGBUILD` and the like), or a diff, a JSON, an XML or an INI file by its first lines. Any other file opens as plain text.

#### Source view

- Heading lines are bold in the source view, as they are on the page. A code font without a bold face shows no difference.

#### Line numbers

- A new `line numbers` row in the settings (`Ctrl+,`) numbers the lines in the left margin: in code files, in text files, and in a markdown file while you edit it. Off by default, remembered once set. The rendered markdown page and the PDF stay without numbers.
- The numbers are in the existing margin, so the text does not move when you turn them on. When the numbers need more room (a very long file, a narrow window, a big zoom), the text moves right by the missing width.
- A line that wraps gets its number on its first row only. The numbers are not copied. In the editor, the number of the caret's line is brighter.

#### Go to line

- `Ctrl+G` opens a small field in place of the search bar. Type a line number and press Enter, and Oryx goes there. `412:10` goes to the tenth character of line 412. A number past the end goes to the last line.
- In the editor the caret goes to the line. While reading a code or text file, the line is shown at the top. On a rendered markdown page, the block that contains that line is shown at the top. `Alt+Left` goes back to where you were. Books and comics have no lines, so the key does nothing there.
- From a terminal, `oryx main.rs:412` and `oryx main.rs:412:10` open the file at that position. It is the form compilers print. A file named `notes:412` still opens.

#### Word count

- A new `word count` row in the settings (`Ctrl+,`) shows the size of the file in the bottom right corner: words, characters, lines and reading time. Off by default, remembered once set. Select some text and the line shows the figures of the selection.
- The count reads the text as the page shows it: markdown marks, link addresses, HTML tags and frontmatter are not counted. A line ending with two spaces counts as a line, a plain line break inside a paragraph does not. Reading time is 240 words a minute.
- A code file shows its lines and characters only. Books and comics show nothing.

#### Autosave

- Two new rows in the settings (`Ctrl+,`) save your file without `Ctrl+S`. `save on focus loss` writes it when you switch to another window. `save after a pause` writes it once you have stopped typing for the time you choose: 5, 15 or 30 seconds, or 1, 5, 10 or 15 minutes. Both are off by default. Fast typing causes one write, and nothing runs while Oryx is idle.
- An automatic save does not overwrite a change made outside Oryx: if the file changed or was deleted on disk, Oryx says so and waits for your `Ctrl+S`. It also waits while the unsaved-changes question is on screen. A note (`Ctrl+M`) has no file yet and is not autosaved.
- The caret is hidden while the Oryx window is in the background.

#### Notes

- A note (`Ctrl+M`) is recovered after a crash, a power cut or a lost display. Oryx copies the note's text to a separate file five seconds after you stop typing, and when the focus leaves the window. The copy is not a save: the unsaved dot stays, and Oryx still asks before closing.
- If Oryx ended without asking, the next launch says "A note from your last session was not saved." `R` or Enter recovers the note, `D` discards it, and Escape leaves it for the next launch. The note comes back unsaved, in this window, or in a second window when Oryx was started on a file.

#### Welcome page

- The welcome page shows a tip, a different one at each launch, from a list that covers the features of Oryx. The tips alternate between the areas (files, moving around, search, editing, themes, export, markdown, books). A "more tips" link under it shows another tip. There is nothing to close or switch off: the tips are only on the page you see when no file is open.

### Fixes

- Searching for a space or a tab now highlights the ones at the end of a line, and the ones where a long line wraps. Before, the count was right but those matches were not highlighted, and Enter could go to a match you could not see. A selection that reaches the end of a line covers its trailing spaces too.
- `\n` in regex mode finds the line breaks. Before, a pattern made only of line breaks found nothing. Replace can join lines, collapse blank lines or add a line after every line, and copy gives the line breaks. A match never crosses from one block to the next, and the file's final line break stays.
- Empty lines added at the end of a file are now shown. Before, each Enter at the end of a long file added a line the page did not show. The caret left the screen and you could not scroll to it.
- The page no longer moves by a pixel on every keystroke at a fractional display scale, such as 125%.
- `Ctrl+B`, `Ctrl+I` and `` Ctrl+` `` work the way you expect: press the key, type, press it again and go on in plain text. Before, the second press wrapped the last word once more and left extra stars. At the end of a word, the key leaves the caret after the closing mark, so you can continue typing.
- The same keys remove the marks around the caret whatever the number of words between them. Before, only a single marked word could be unmarked. Pressed twice on an empty spot, the key removes the empty pair it just made.
- `Tab` in a new markdown file inserts four spaces, which nest under every list marker. Before, it inserted a tab character. A file that already indents with tabs or with two spaces keeps its indentation, and a new code or text file still gets a tab.
- In a markdown file, `Tab` no longer turns a list into a code block. A list item nests under the item above it, one level at a time. With no item above, or when the item is already nested, `Tab` leaves the list alone. Before, `Tab` on a first-level list put four spaces in front of it, which markdown reads as code.
- `Tab` on a list item inside a quote (`> - item`) nests it inside the quote, and `Shift+Tab` brings it back. Before, the spaces went in front of the `>`, and markdown read the line as plain text.
- In a markdown file, `Ctrl+E` and `Escape` keep the line you are on at the same height on the screen. Before, the line jumped to the top of the window each time.
- The caret comes back on the line it left when that line is an image, an empty line or a rule. Before, it came back on the paragraph above, or on the first line of the file when a tall image filled the window.
- `Up` on the first line goes to the start of the line, and `Down` on the last line to its end. With Shift, the selection follows.
- Oryx no longer keeps a processor core busy after you leave the editor with Escape.
- When the connection to the display is lost (a compositor that crashes or drops the window), Oryx closes cleanly, saves its settings and says why in the terminal. Before, it could crash while closing, mostly while a big file was loading.
- Fixed a rare crash while a big markdown file with many code blocks was still loading. It was seen on an 8 MB file opened at a far line.
- A code or text file you come back to in the same session reopens at the line you were reading. Before, only markdown files and books kept their place.
- The sidebar opened from a new note (`Ctrl+M`) shows the folder you were in. Before, it showed Oryx's own folder for notes.
- `Ctrl+Shift+S` works on a file you are only reading. Before, it did nothing until the file had been in the editor once.
- The file Oryx was started with reloads when it changes on disk, like any file opened later.
- A file whose lines end in CR CR LF (an old Mac file converted to Windows, or converted twice) no longer breaks the editor. Before, each typed letter pushed its line down on screen, and Oryx soon crashed. Each return before a line break is kept and written back on save, so an untouched line is not changed. A new line takes the file's usual ending. Pasted text is cleaned the same way. A classic Mac OS file, whose lines end in CR alone, reads as lines too and saves with CR again. Before, it opened as one long line.
- A binary file whose first zero byte is after the first 8 KB is now refused, like any other binary. Before, a cartridge ROM opened as text, and its random characters made Oryx load all the system fonts: 700 MB of memory and a freeze of ten seconds. A file is binary when its first 8 KB holds a zero byte or is more than 30% unreadable bytes. A text file in an encoding older than UTF-8 (Cyrillic in CP1251, Japanese in Shift-JIS) is refused the same way, with the message "is not UTF-8 text".
- Control characters in a text file (the escape codes of a log, for example) no longer freeze Oryx. They show as spaces. Before, each one made Oryx search every font on the system for a glyph: a log of 190 KB took a minute and a half to open. It now takes a fifth of a second.
- A fresh install opens with the sidebar visible, on your home folder. Before, the sidebar was closed, and once opened it showed the folder Oryx was started from, which on Windows can be a system folder. Close it once and it stays closed. The open and export dialogs start in your home folder the same way.
- A folder Oryx cannot read shows one row saying so, under the `..` row, so you can go back up. Before, the sidebar was blank.
- The welcome page says that the sidebar key shows and hides the panel.
- The folder you are in is shown in the accent color in the sidebar, its name and its icon: the folder of the open file, or, with no file open, the folder you last clicked. Before, the folder you had opened and the folder under the mouse looked the same.
- In a right-to-left page, `align="center"` and `align="left"` move the text. Before, it stayed on the right.
- The checkboxes of task lists are a little bigger, and the check mark fills the box. The click area and the PDF are updated too.
- In the font list of the settings, a click on the hint under the list no longer picks a font you could not see.

## v1.1.1

A small release for the first two reports from a user, issues #1 and #2 on GitHub. The details, by area:

### Page breaks

- You can now force a page break in the PDF. Write `<div style="page-break-after: always"></div>` or `\newpage` on a separate line. The PDF starts a new page there, and the window shows a dashed line. `\pagebreak` and `\clearpage` work too. SYNTAX.md lists the accepted forms.
- Two breaks in a row make one new page. A break at the end of the file adds no empty page.

### Source view

- The `---` of a rule and the `>` of a quote were nearly invisible in most themes when a file showed as source (Ctrl+E). They took the color of the thin line they draw. They now use the punctuation color, and quoted text shows in the normal text color.
- Bold and italic inside a quote keep their colors in the source view.
- Code comments were too faint to read in eleven themes. Each comment color moved one step darker or lighter, and a test now checks that comments are readable in each theme.

## v1.1.0

This release is about the editor and the markdown it reads. New in this release: more editing shortcuts, a quick note on Ctrl+M, search fields that behave like text boxes, a new look for the sidebar and for the unsaved changes dialog, and the extended markdown syntax of the Markdown Guide. The details, by area:

### Lists and headings

- Enter on an empty list item ends the list. Before, if you had clicked into the item, the dash stayed.
- Alt+- turns the selected lines into a bullet list, Alt+1 into a numbered list and Alt+X into a task list. Press the same key again to remove the markers. One Ctrl+Z undoes it.
- Alt+. quotes the selected lines. Press it again to remove the quote.
- Ctrl+1 to Ctrl+6 set the heading level of the line. The same level again makes it plain text.
- Ctrl+L ticks or unticks the task box of the line, or of every selected line that has one.
- Enter in the middle of a numbered list renumbers the items below. One Ctrl+Z undoes the new line and the numbers together.

### Text and lines

- Type `(`, `[`, `{`, `"` or `'` with text selected and Oryx wraps the text instead of replacing it. In a markdown file `*`, `_` and a backtick wrap too, so two stars make bold.
- Ctrl+B, Ctrl+I and Ctrl+` make the selection or the word under the caret bold, italic or code. The same key again removes it.
- Ctrl+K turns the selection into a link and puts the caret where the address goes. Pasting an address over selected text makes a link too.
- Alt+Up and Alt+Down move the line, or the selected lines, up and down.
- Ctrl+Shift+D duplicates the line or the selected lines. Ctrl+Shift+K deletes them.
- Ctrl+/ comments or uncomments the line or the selected lines, in the language's own way. In a markdown file it uses an HTML comment.
- A keyboard selection can now reach the empty line at the end of the file.
- Syntax colors arrive while text is selected, and right after a shortcut moves or rewrites lines. Before, those lines stayed uncolored until you cleared the selection.
- Shortcuts with a digit, a period or a dash now work on a French keyboard, where those keys need Shift.

### The caret

- A click on an empty line puts the caret there. Before, only the arrow keys could reach an empty line.
- A click or an arrow key past the end of a line puts the caret after the trailing spaces, as End does.
- The caret shows on an empty file, and a click on an empty page places it.

### Search

- The search and replace fields behave like text boxes. Ctrl+Left and Ctrl+Right jump words, Shift selects them, Ctrl+Backspace and Ctrl+Delete remove a word, Ctrl+Y redoes. A click places the caret, a double click selects the word, a triple click selects everything, and a drag selects.
- While a search field has the focus, the editing keys no longer change the document. Before, Ctrl+B or Tab typed in the search field changed the document. The fields of the theme editor and the theme browser have the same keys.
- The search bar's text is now aligned with the caret and the regex button.

### Notes and saving

- Ctrl+M opens an empty markdown note in the editor, with no dialog. You choose the name, the place and the type at the first Ctrl+S. Until then the note is unsaved work, and Oryx asks the usual question before closing or opening another file.
- After Save As, the sidebar shows the folder you saved in.

### Markdown

- Heading IDs: `## Title {#custom-id}` gives the heading that anchor, and the braces are not shown in the title.
- Definition lists: a term line followed by `: definition` lines renders like the HTML form, the term bold and the definitions indented.
- `H~2~O` and `x^2^` render as subscript and superscript. A tilde between spaces still strikes through, as on GitHub.
- `==text==` and `::text::` highlight the text.
- Abbreviations: a `*[HTML]: Hyper Text Markup Language` line marks every HTML in the document with a dotted underline, and hovering the word shows the expansion.
- Footnotes are numbered in order of use, as on GitHub, whatever their labels. A footnote of several paragraphs shows its number once, with the other paragraphs indented under it. Before, the label repeated on every paragraph.
- The number at the foot of a footnote links back to where it is cited, in the window and in the PDF.
- The PDF export includes all of the above.
- SYNTAX.md was rewritten. Every construct is shown twice, as written and as rendered.
- Big markdown files take less memory. An 8 MB markdown file now uses 172 MB instead of 201 MB, and its peak while loading is 30 MB lower.

### The look

- Redesigned the sidebar. Folders have a small triangle and an outlined folder icon. Files have an outlined icon by type. Children are linked by thin guide lines. The open file takes the theme's accent color, with a bar at its left. The row under the mouse is highlighted, and the list has a scrollbar, for the files and for the outline. No theme needs a change.
- Redesigned the unsaved changes dialog in the same style. The page dims behind it and the file's name is shown. The three answers are rows with their key beside them: S saves, D discards, Escape keeps editing. The arrows move between the rows and Enter or Space picks the highlighted one. A click on a row picks it, and a click outside keeps editing.
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
- Oryx is in the Microsoft Store as Oryx Editor, free, for Windows 10 and 11. The Store package cannot claim the file types Windows reserves for itself (`.bat`, `.cmd`, `.js`, `.pl`, `.py`, `.rb`), so with the Store install those files are opened from inside Oryx (Ctrl+O or the sidebar), not from the file manager. The MSI registers all the types.
- On Windows, `winget install Steerania.Oryx` installs the MSI.
- Headings and bold words stay in the chosen body font when that font has no bold face (a cursive or display font, for example). Before, they switched to another font, on screen and in the PDF.
- In the editor, a checkbox followed by a word in parentheses (`- [ ] (Ivan) call back`) no longer shows the parentheses in the link color. The old Markdown allowed a space between `[text]` and `(url)`. The page does not, and the editor now does the same for links, references and images.
- The Linux binary is built on Ubuntu 22.04 and runs on Debian 12, Ubuntu 22.04, Fedora 36, openSUSE Leap 15.4 and newer (glibc 2.35, OpenSSL 3).
- The Windows installer is 9 MB instead of 26. The old one contained the program twice.
- On Linux, Oryx registers itself as `com.steerania.Oryx`, so the window icon shows on Wayland desktops. An earlier registration is cleaned up. Under a package, `oryx --register` says the package already did it.
- Themes are also found in the system folders where a package installs them (`/usr/share/oryx/themes` and the other `XDG_DATA_DIRS` entries).
- Clicking a heading in the outline goes to that heading, not to the first one with the same text. Repeated headings get numbered anchors as on GitHub (`#title`, `#title-1`).
- A privacy page, `PRIVACY.md`: no account, no telemetry, what is kept on the machine and where.
- The `F1` page links to the documentation on GitHub, the project's home.
- The welcome page points to the settings (`Ctrl+,`, for the fonts, the sizes and the interface scale) and to the documentation on GitHub. The `F1` page says what `Ctrl+,` opens, what `Left` and `Right` do (move to the sidebar and back), and what `Enter` does in the sidebar.

## v0.15.8

A review of the whole program before publishing it wider: two dozen fixes and small additions, plus two more reported while testing. The six fixes that could lose work or hide content come first.

- A file with a very long line (a minified script, a one-line JSON) no longer freezes Oryx. A 250 KB line went from 13.8 seconds to 53 milliseconds to open.
- Reloading a file that got shorter while it was being edited no longer crashes. The caret stays inside the file.
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
- Remote images older than a day are refreshed in the background. `Ctrl+Shift+R` reloads the page and fetches its remote images again. `oryx --clear-cache` empties the image cache.
- Three crashes on malformed books are fixed. Each now shows an error message.
- The settings file is written safely, and a value edited out of range by hand is corrected instead of breaking the display.
- A folder reached through a symbolic link appears in the sidebar. A click on it opens that folder in the sidebar.
- Pictures in a markdown file now share the memory budget books already had, and a camera photograph is stored no larger than 4096 pixels on its long side: a page of twelve 24-megapixel photos holds 107 MB instead of 1,177 MB.
- A missing inline image shows its alt text, or its file name, inside its box.
- `Escape` clears the selection before it quits.
- The zoom shortcuts work on every keyboard layout, AZERTY included.
- The dependencies are audited before each release. The two advisories the first audit found are no longer in the build.
- On Windows, cancelling the export dialog with `Escape` no longer closes Oryx with it.

## v0.15.7

Oryx now reads Arabic and Hebrew books, right to left, in fonts made for them.

- The direction is detected paragraph by paragraph from the text itself, so a book mixing English and Arabic shows each paragraph on its correct side. Justified Arabic stretches to both edges with the last line ending on the right, like print.
- Two fonts are embedded: Amiri for Arabic, a revival of the typeface classical Arabic books were printed in, and David Libre for Hebrew. They are used for their scripts with any body font.
- Lists, quotes and headings follow the direction of their text: bullets, numbers and quote bars move to the right side of a right-to-left block.
- Selecting, copying and searching work in right-to-left text as in any other, and the PDF export keeps the direction, the fonts and copyable text.
- `Ctrl+D` switches the reading direction (automatic, right to left, left to right) and is remembered per book. In the two-page comic view it flips the page order, for manga.
- Tested against the full library, now including every EPUB: 2,689 of 2,697 books opened and read correctly in 75 seconds (the eight failures are broken downloads, damaged files, or fixed-layout books, each refused with a clear message).
- The two fonts add about 1.1 MB to the binary. The rest of the feature is about 22 KB of code, with no new external library.

## v0.15.6

Oryx now reads comic books, and `Alt+Left` goes back after any jump.

- CBZ and CBR comics open with their pages in order. A comic starts as a vertical strip at the window's width. `Ctrl+Minus` switches to one whole page per screen, then to two pages side by side like an open book, `Ctrl+Plus` goes back up, and `Ctrl+0` shows the whole page from anywhere. In the page views, `Up`, `Down` and `Space` turn pages.
- Comics are opened by their content, not their file name, so a `.cbr` that is really a zip works anyway. Password-protected and damaged archives are refused with a clear message, and so is the rare CBR with compressed pages (almost all store them, which Oryx reads).
- `Alt+Left` goes back after clicking a link, a footnote or an outline entry, in every kind of document, one jump at a time.
- Fixed: a reading position saved at the very top of a markdown file was lost.
- Comics open fast whatever their size: a 78 MB, 192-page book opens in under 50 ms. Tested against a real library: 240 of 242 books and comics opened correctly in under 9 seconds (the two failures are damaged files known from the last release).
- The RAR reading was written from scratch for Oryx and checked byte for byte against the reference unrar tool. All of this adds about 52 KB to the binary and no new external libraries.

## v0.15.5

Oryx now reads three more book formats: FB2, MOBI and AZW3 (Kindle). These formats have the same features as EPUB: the active theme, the outline panel, working links and footnotes, remembered reading positions, and PDF export with chapters and bookmarks.

- FB2 books open with their pictures, poems and footnotes. Files saved as `.fb2.zip` or `.fbz` work too, and so does the windows-1251 encoding common in Russian books.
- MOBI and AZW3 books open with their pictures, chapter links and table of contents. A file carrying both the old and the new Kindle format inside uses the newer one. DRM-protected books are refused with a clear message, and a damaged file is reported as damaged.
- The window title shows the book's format next to its title, so the same book in EPUB and in MOBI can be told apart.
- Reading positions are remembered per format: leaving the MOBI of a book does not move your place in its EPUB.
- Tested against a real library: 231 of 233 FB2, MOBI and AZW3 files opened correctly, in 10 seconds total (the two failures are damaged files). The same book costs about the same to open whichever format it comes in.
- All of this adds about 150 KB to the binary and no new external libraries. The Kindle format support was written from scratch for Oryx.

## v0.15.4

Oryx can now edit what it reads. It was built in four milestones. Books are read-only.

**Milestone 4, find and replace**

- Search can use regular expressions: press the `.*` button in the search bar, or `Alt+R`. Capture groups, backreferences and lookarounds all work (the Rust `fancy-regex` flavor). `^` and `$` match line by line, and on the rendered page every block counts as a line. Works while reading too.
- `Ctrl+H` opens find and replace in the editor. `Enter` replaces the current match and moves to the next. `Ctrl+Enter` replaces every match at once, as a single undo step. A replacement can reuse captures: searching `(\w+)/(\w+)` and replacing with `$2/$1` swaps every pair in the file.
- The search boxes behave like real text boxes now: their own undo and clipboard, a monospace font, and `Ctrl+Home`, `Ctrl+End` and the word jumps pass through to the document. Clicking anywhere else closes the bar, like the dialogs.
- The editing mode is now visible: the title says `editing`, a thin line in the theme's selection color runs along the top of the page, and the unsaved marker is now a dot before the file name instead of an asterisk.
- Fixed on the way: searching `f` could highlight `fi` (the font draws them as one shape, and the highlight now splits it), and a search for an empty pattern could hang.

**Milestone 3, everyday editing habits**

- `Enter` keeps the line's indentation, and in a markdown file it continues what is being written: list markers repeat, numbered lists count on, task items continue unchecked, quotes keep their `>`, and `Enter` on an empty item ends the list.
- `Tab` indents and `Shift+Tab` removes an indent, on every line of a selection at once. With the caret at a list marker, `Tab` nests the item. Whether `Tab` writes a tab or spaces follows what the file already uses.
- Task checkboxes tick by a click on the rendered page, without entering the editor. Nothing else on the page moves, `Ctrl+Z` undoes it (undo now works while reading too), and `Ctrl+S` saves it.
- `Escape` quits without closing the sidebar first. `Ctrl+Shift+B` still hides the sidebar, and the sidebar comes back as you left it.
- The dialogs improved too: a title band colored from the theme, and a click outside closes any of them.
- Fixed: opening the help page while editing a markdown file blanked the outline to "No headings" for the rest of the edit.

**Milestone 2, markdown editing**

- Markdown files can be edited. `Ctrl+E` shows the file's own markdown in the editor, `Escape` returns to the rendered page. Everything the editor already does applies: typing, selection, undo and save.
- The markdown source is drawn in the theme's own colors: headings in their ramp, `**bold**` in the bold color with its markers, inline code, links, quotes and rules likewise, with real bold and italic faces that keep the columns aligned.
- The editor opens at the passage being read and the page returns to the line being edited, in both directions.
- Coming back without changing anything is instant at any file size. The page is kept, not rebuilt (measured on an 8 MB file).
- While editing, the outline keeps the page's headings, a click moves the caret to that heading, and a save refreshes the list.
- Switching between files keeps each file's place for the session: a file left mid-edit reopens in the editor at its caret, a file left reading reopens at the same passage.
- The `F1` page names the running version and links to the full documentation on Codeberg and GitHub.

**Milestone 1, plain text editing, and fast coloring of huge files**

- Editing begins. `Ctrl+E` edits code and plain text files on the page itself: typing, `Enter`, `Tab`, deletion, selection with `Shift` and the mouse, cut, copy and paste. `Escape` returns to reading. Markdown editing comes in a later version. Books are read-only.
- Undo with `Ctrl+Z`, redo with `Ctrl+Shift+Z` or `Ctrl+Y`. Text typed without a pause is undone together. A pause or a jump starts a new step.
- `Ctrl+S` saves. A save never changes a byte you did not touch, line endings included, and a test suite proves it on every build.
- Typing is instant at any file size: a keystroke in an 8 MB file cost about 2 seconds of relayout during development, under 20 ms now.
- `Ctrl+N` creates a new file, `Ctrl+Shift+S` saves under a new name. Closing, quitting or reloading with unsaved changes asks first: `Enter` saves, `D` discards, `Escape` cancels.
- The file reloads by itself when it changes on disk. With unsaved edits, a notice in the corner reports the change.
- The caret moves like in a text editor: `Ctrl+Left` / `Ctrl+Right` jump by word, `Ctrl+Home` / `Ctrl+End` jump to the ends, `Ctrl+Backspace` / `Ctrl+Delete` delete by word.
- `F1` opens a help page rendered by Oryx itself: searchable, themed, scrollable. `F1` or `Escape` returns where you were, with your unsaved edits. The old panel is removed.
- Markdown text can justify like a book: `Ctrl+J`, remembered separately for markdown (off unless you ask) and books (on). The PDF export follows the same choice.
- Code files draw edge to edge with the theme's code background as the page, no frame around the file.
- Text files show every line the file has, blank lines included, like an editor.
- The title bar shows `*` while there are unsaved edits.
- Shortcuts changed: the sidebar toggle is `Ctrl+Shift+B`, the sidebar tab switch is `Ctrl+Tab`.
- Syntax colors reach the part of a big code file you are looking at right away. Jumping to the end of an 8 MB file showed plain text for about 36 s. It is now colored in about 9 ms, at any position.
- Editing a big code file no longer re-colors the whole file after every pause: an 8 MB file took about 34 s, now about 60 ms.
- `End` and `Ctrl+End` reach the real end of a file still being laid out: the view settles there once the whole document is placed, unless you scroll somewhere else meanwhile.
- Fix: exporting a very large markdown file to PDF had doubled in time (about 19 s instead of 10 s on an 8 MB file). It was found by the new performance check and fixed.

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
- A book opens without decoding its images, and the page no longer shifts as images arrive.
- Scrolling fast into a new chapter can leave an image blank for a fraction of a second.
- Fix: EPUB 2 books show their real table of contents again (for most, Oryx used a scan of the chapter headings).
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
- The binary grows about 1.2 MB for EPUB support.

## v0.13.1

- The export shortcuts moved: `Ctrl+P` exports, `Ctrl+Shift+P` opens the export settings. `Ctrl+E` is reserved for the editing mode planned later.
- The PDF export has an orientation setting: portrait or landscape, remembered.
- In the export settings, clicking the Export row starts the export (before, only Enter worked).
- In the theme browser, the arrow keys preview themes as they move through the list. Enter validates, Escape cancels.
- The sidebar tab headers are no longer painted over when a long outline scrolls.
- The README got a new hero image, a cleaner top section, and plainer wording overall.

## v0.13.0

- Math is now typeset: fractions, roots, superscripts and subscripts, big operators with their limits, and stretching delimiters, in the STIX Two Math font.
- All four GitHub math notations work: `$...$`, `$$...$$`, the ` ```math ` fence, and inline `` $`...`$ ``.
- The command coverage follows KaTeX: Greek letters, arrows, relations, accents, the math alphabets like `\mathbb{R}`, operator names, matrices, `cases` and `aligned`.
- `\newcommand` defines macros with arguments, usable in the same equation.
- If Oryx encounters a command it does not know, it renders it as its literal source, and the rest of the equation still typesets.
- Oryx infers whether a dollar sign is a currency or math, so prices like `$5-$10` stay text.
- A wide equation shrinks to fit the page, and the line of a tall inline equation grows to fit it.
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
- A selection is kept after a zoom, a theme switch or a window resize.
- Find covers the whole document while it is still loading, and a phrase now matches across a wrapped line.
- Copying a document as markdown no longer stops at the footnote definitions.
- Documents with emoji export again: emoji embed as images in the PDF.

## v0.10.0

- Big markdown files open instantly: Oryx parses only the first screens before the first paint and the rest arrives in the background. An 8MB file drops from about 440ms to 80ms.
- Layout uses all CPU cores: the coloring below the first screens, zoom and resize are two to three times faster.
- PDF export of the 8MB test file drops from 17s to under 10s.
- Select all on a huge file no longer freezes the app, and a selection is kept while syntax colors are still arriving.

## v0.9.0

- The binary got smaller.
- Big files stay fast: hovering, scrolling and search are instant even on an 8MB document.
- PDF export fixes: system fonts embed correctly, images and code blocks are no longer cut between two pages, tall table rows paginate properly.
- Badges can now load behind a proxy on Windows, and a corrupted image cache is repaired automatically.
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

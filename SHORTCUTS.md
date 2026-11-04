# Oryx shortcuts

Oryx has no menus: you use it with the keys of this page, and with the mouse. The same list opens inside Oryx with `F1`, together with the markdown syntax.

On macOS, use `Cmd` where this page says `Ctrl`, and `Cmd+[` / `Cmd+]` in place of `Alt+Left` / `Alt+Right`. The `Alt` key is the Option key: a shortcut with `Alt` holds by the key printed on the keycap, and `Alt+1` and `Alt+.` by the place of their key as well. Option with Shift, or with a key that types a plain character, types it as usual. `Alt+R` works in the search bar only: elsewhere, Option+R gives ®.

On a keyboard layout that types another script, such as Cyrillic or Greek, or an accented letter on a letter key, such as BÉPO, a letter shortcut holds by the place of its key: `Ctrl` with the key of C is `Ctrl+C`. The shortcuts with a digit or a punctuation key (the zoom, the headings, the lists, the quote, the comment, the code span and the settings) hold by the place of their key as well.

## Files

| Key | What it does |
|---|---|
| `Ctrl+O` | Open a file |
| `Ctrl+N` | New file; the save dialog opens first, so Oryx knows the type of the file |
| `Ctrl+M` | New markdown note; the name and the place are chosen when you save |
| `Ctrl+S` | Save (editing) |
| `Ctrl+Shift+S` | Save as |
| `F5` / `Ctrl+R` | Reload from disk |
| `Ctrl+Shift+R` | Reload and download the remote images again |

## Moving around

| Key | What it does |
|---|---|
| `Up` / `Down` | Scroll by line, or move the selection in the sidebar |
| `Page Up` / `Page Down`, `Space` / `Shift+Space` | Scroll by page |
| `Home` / `End` | Go to the top / the bottom |
| `Alt+Left` / `Alt+Right` | Go back after a jump (a link, a search result, a line, another file), and forward again |
| `Ctrl+G` | Go to a line: `412`, or `412:10` for column 10 |
| `Ctrl+Shift+B` | Show or hide the sidebar (files and outline) |
| `Left` / `Right` | Move to the sidebar / to the document |
| `Ctrl+Tab` | Switch the sidebar between files and outline |
| `Ctrl+Shift+H` | Show or hide hidden files in the sidebar |
| `Enter` | In the sidebar, open the selected file |
| `Ctrl+Enter` | In the sidebar, open the selected file in a second window |

## Find

| Key | What it does |
|---|---|
| `Ctrl+F` | Find in the document |
| `Enter` / `Shift+Enter`, `F3` / `Shift+F3` | Next / previous match |
| `Alt+R` | Regular expressions on or off |
| `Ctrl+H` | Find and replace (editing) |
| `Tab` | With the replace field open, move between the find and the replace fields |
| `Enter` | With the replace field open, replace the current match and go to the next one |
| `Ctrl+Enter` | With the replace field open, replace every match; one `Ctrl+Z` brings them all back |

## Selection and copy

| Key | What it does |
|---|---|
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy the selection with its formatting, so it pastes into an email or a Word document; in the editor, copy the line when nothing is selected |
| `Ctrl+Shift+C` | Copy the selection as markdown |

## Editing

| Key | What it does |
|---|---|
| `Ctrl+E` | Switch between reading and editing |
| `Escape` | Back to reading |
| `Ctrl+X` | Cut the selection, or the line when nothing is selected |
| `Ctrl+V` | Paste; a line cut or copied whole goes back above the current line |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` / `Ctrl+Y` | Redo |
| `Ctrl+Left` / `Ctrl+Right` | Move by word |
| `Ctrl+Home` / `Ctrl+End` | Go to the start / the end of the file |
| `Ctrl+Backspace` / `Ctrl+Delete` | Delete a word |
| `Alt+Up` / `Alt+Down` | Move the line or the selected lines up / down |
| `Ctrl+Shift+D` | Duplicate the line or the selected lines |
| `Ctrl+Shift+K` | Delete the line or the selected lines |
| `Ctrl+/` | Comment or uncomment the line or the selected lines, in the style of the language |
| `Ctrl+U` | Change the case of the selection or the word, in turn: lower case, a capital on each word, upper case |
| `Tab` / `Shift+Tab` | Indent / remove an indent, on every selected line |

Closing, quitting or reloading with unsaved changes asks first: `S` saves, `D` discards, `Escape` goes back to editing.

## Markdown editing

| Key | What it does |
|---|---|
| `Ctrl+B` / `Ctrl+I` | Bold / italic on the selection or the word; again to remove |
| `` Ctrl+` `` | Inline code on the selection or the word; again to remove |
| `Ctrl+K` | Link on the selection or the word; pasting an address over a selection links it too |
| `Ctrl+1` to `Ctrl+6` | Heading level of the line; the same level again to remove it |
| `Alt+-` | Bullet list on the selected lines; again to remove |
| `Alt+1` | Numbered list on the selected lines; again to remove |
| `Alt+X` | Task list on the selected lines; again to remove |
| `Ctrl+L` | Tick or untick the task of the line |
| `Alt+.` | Quote the selected lines; again to remove |
| `Enter` | New line that continues the list, the task list or the quote; on an empty item, end the list |
| `Shift+Enter` | Line break inside a paragraph or a list item |
| `Tab` / `Shift+Tab` | On a list item, nest it under the item above / bring it back |
| `Ctrl+V` | Paste a picture: Oryx saves it in an `images` folder next to the file and writes the link |
| `Ctrl+Shift+V` | Paste a picture at its full size (by default, big pictures are reduced to 2560 pixels) |

With text selected, typing a bracket, a quote, `*`, `_` or a backtick wraps the selection instead of replacing it.

## View

| Key | What it does |
|---|---|
| `Ctrl+T` | Choose a theme: arrows to preview, `Enter` to keep, `Escape` to go back |
| `Ctrl+,` | Settings: fonts, sizes, interface scale, line numbers, word count and autosave |
| `Ctrl+Plus` / `Ctrl+Minus` | Zoom in / out; in a comic, switch between page width, whole page and two pages |
| `Ctrl+0` | Reset the zoom; in a comic, show the whole page |
| `Ctrl+J` | Justify the text on or off (markdown and books) |
| `Ctrl+D` | Reading direction: automatic, right to left, left to right; in a comic, the page order |

## Export

| Key | What it does |
|---|---|
| `Ctrl+P` | Export to PDF with your export settings |
| `Ctrl+Shift+P` | Choose the export settings (theme, fonts, page size), then export |

## Help

| Key | What it does |
|---|---|
| `F1` | Open and close the help page: every shortcut and the markdown syntax |
| `Escape` | Close what is open, clear the selection, leave editing, quit |

## Mouse and touch

| Action | What it does |
|---|---|
| Drop a file on the window | Open it |
| Drop a folder on the window | Show it in the sidebar |
| Drop a picture on a markdown file you are editing | Add it to the file |
| Double click | Select a word and highlight every other place it appears |
| Triple click | Select the paragraph, the line of code or the table cell |
| `Shift` + click | Extend the selection to the click |
| Drag over several words on one line | Select them and highlight every other place the text appears |
| Drag past the top or the bottom of the window | Scroll the page and keep selecting while you hold the button |
| Click on a task box | Tick or untick it, without entering the editor |
| Middle click on a file in the sidebar | Open it in a second window |
| `Ctrl` + mouse wheel | Zoom |
| Back and forward mouse buttons | Same as `Alt+Left` / `Alt+Right` |

On a touch screen, a swipe scrolls, a tap clicks, and a pinch with two fingers zooms.

## Command line

On Linux, the command is `oryx`. On Windows and macOS, Oryx is called by its full path, given in [INSTALL.md](INSTALL.md#from-a-terminal).

```sh
oryx README.md          # open a file
oryx notes/             # open the sidebar on a folder
oryx main.rs:412        # open a file at line 412
oryx main.rs:412:10     # at line 412, column 10
git diff | oryx         # show what a command prints; oryx - reads standard input
git log | oryx --as md  # tell Oryx what kind of text it gets
oryx --theme nord file  # use a theme for this session only
oryx --register         # register the file types and icons
oryx --clear-cache      # remove the downloaded remote images
oryx --version          # print the version
oryx --help             # list these options
```

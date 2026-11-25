# Input method field test

The tests by hand of the "Input method" setting, on a real session. They
are run again after each change of `src/input/method.rs` or of the
`Ime` events in `src/app.rs`. The tests that need no hand are in
`tests/input_method.rs`.

Work on a copy of this file, since the steps type in it. Open the copy
with Oryx and turn the setting on: `Ctrl+,`, the row "Input method",
`Space`, `Escape`.

While you compose, the composing text shows in a box at the caret,
underlined, and the candidates open beside the box. The text the steps
type in is the last three lines of text of this file, so `Ctrl+End`
goes to it.

## H1. fcitx5 on Wayland

About 10 minutes. "Pinyin on" means that the icon of fcitx5 shows
Pinyin. `Ctrl+Space` switches.

1. `Ctrl+E`, `Ctrl+End`, Pinyin on. `nihao`, `Space`: 你好 is typed
   after `end`.
2. `nihao`, `Enter`: the letters `nihao` are typed, with no new line.
3. `nihao`, `Escape`: nothing is typed, and the editor stays open.
4. `nihao`, `Left`, `Left`: the bar moves inside the box, with a
   stronger line up to it. `Space`.
5. Double click on `gamma`, then `ni`, `Space`: 你 replaces `gamma`.
6. `ni`, then a click on the line `one two three four`: `ni` is typed
   where the caret was, and the caret goes to the click.
7. `ni`, then `Ctrl+F`: the search bar opens. The next letter shows in
   a box in the bar. `Escape` until the bar is closed.
8. `ni`, then `Ctrl+G`: the box goes, and nothing is typed. `5`,
   `Enter`: the caret is on line 5. `Ctrl+End`.
9. `ni`, then `Alt+Tab` to another window and back: `ni` is typed.
   Then `nihao`, `Space`: 你好.
10. `ni`, then scroll to the top of the file with the wheel, then `h`:
    the view comes back to the box. `Escape`.
11. `Escape` to reading mode, `Ctrl+F`, `nihao`, `Space`: 你好 is in
    the bar and its matches are lit. `Escape`.
12. In reading mode, `Space` and the arrows move the page, and no
    candidate opens.
13. A keyboard chosen in fcitx5 (Russian): in the editor the letters
    are typed, and `Ctrl+S` saves with the key at the place of `S`.
14. The candidates: beside the box in the editor, and under the box in
    the search bar. They are not at a corner of the window.
15. The setting off, then steps 1 and 11 again: the letters are typed
    as letters.

## H2. fcitx5 under X11

About 5 minutes. Launch with `WAYLAND_DISPLAY` empty and
`XMODIFIERS=@im=fcitx`. fcitx5 draws the composing text itself there:
Oryx shows no box, and its caret stays.

1. `Ctrl+E`, `Ctrl+End`, Pinyin on. `nihao`, `Space`: 你好 is typed.
2. `ni`, then `Alt+Tab` to another window and back: nothing is typed.
   Then `nihao`, `Space`: 你好, with no `Ctrl+Space` before it.
3. The candidates open at the caret. Open another file from the
   sidebar, `Ctrl+E`, `ni`: they open at the caret of that file.
4. Scroll the editor with the wheel, the caret in view: the scroll
   feels the same with the setting on and off.

## H3. Windows

About 25 minutes. "Pinyin on" means that the taskbar shows 中. `Shift`
switches.

1. Microsoft Pinyin: steps 1 to 5 and 9 to 11 of H1. At step 4 the bar
   moves and there is no stronger line. At step 9 the text is typed as
   it shows in the box.
2. Microsoft Pinyin: `ni`, then a click on another line, then `Space`:
   你 is typed on the line clicked.
3. Microsoft Pinyin: `ni`, then `Ctrl+,`, `Ctrl+F` or `Ctrl+G`: nothing
   opens while the candidates are open.
4. Microsoft Pinyin: the comma and the period in Chinese mode type
   `，` and `。`.
5. Microsoft Pinyin: `Ctrl+,`, `Escape`, then `ni`: the candidates are
   beside the caret.
6. Weasel: steps 1 to 3 of H1. Weasel shows the letters in its own
   window, and Oryx's box is one space wide.
7. Japanese, in hiragana: `kyouhaiitenkidesu`, `Space`: the clause
   being converted has the stronger line. `Enter`: the sentence is
   typed.
8. Korean, in Hangul: `dkssudgk`, `Space`: 안녕하 and a space are
   typed, in that order.
9. `Win+.`, a click on an emoji: the emoji is typed.
10. A Russian layout: the letters are typed, and `Ctrl+S` saves with
    the key at the place of `S`.
11. A French layout: the key `[` then `e` types ê.
12. The setting off, then steps 1, 9, 10 and 11 again: typing is as it
    was before the setting.

## The Mac

Not tried. The code follows the source of winit.

## The text

alpha beta gamma delta

one two three four

end

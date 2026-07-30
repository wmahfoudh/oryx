# This is Oryx

A native reader that opens an 8 MB file in 80 milliseconds, keeps memory
flat while you scroll, and exports what you see to a themed PDF. **Bold**,
*italic*, ~~strikethrough~~ and `inline code` share a line politely, a
[link](https://codeberg.org/wmahfoudh/oryx) is a click away, smart
punctuation turns "quotes" curly, and math keeps its symbols inline:
$e^{i\pi} + 1 = 0$.

> [!TIP]
> There are no menus. `F1` lists every shortcut, `Ctrl+T` browses 31
> themes live, and this whole page is colored by one of them[^theme].

| It reads | It shows | It writes |
|---|---|---|
| Markdown and code | Tables, alerts, footnotes, math | A themed PDF |

- [x] Task lists with real checkboxes :sparkles:
- [x] Close to a hundred languages highlighted:

```rust
fn open(file: &Path) -> Reader {
    // An 8 MB document, first paint in 80 ms.
    Reader::new(file).themed("oryx-sand")
}
```

[^theme]: This one is gruvbox-dark. Any theme is one TOML file, editable live.

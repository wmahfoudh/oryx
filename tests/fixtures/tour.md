# Oryx Feature Tour

This document exercises every rendering feature as it lands. Open it after
each task and the newest section shows what just became visible.

## Text Styles

Plain body text with **bold emphasis**, *an italic slant*, ~~struck through
words~~, `inline code`, and a [link to Codeberg](https://codeberg.org). Bold
uses its own color, not only a heavier weight, and the same is true for
italics, so themes control every style independently.

This second paragraph is deliberately long enough to wrap on any reasonable
window size, which makes the proportional margins and the line height rhythm
visible: resize the window and the text reflows while the ten percent margins
on each side hold their ratio. Consecutive lines inside one paragraph sit
flush at exactly one and a half times the font size, while paragraphs are
separated by space scaled to their weight.

## Code

Inline code like `let x = 1` sits in a pill on the shared baseline.

```rust
// Comments are muted; keywords, strings, and numbers each take a hue.
fn fibonacci(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}
```

```python
def greet(name: str) -> str:
    """Docstrings count as strings."""
    return f"Hello, {name}!"
```

```
A fence with no language renders plain, inside the same panel.
```

## Quotes, Lists, Rules

> A quoted paragraph gets its bar and a tinted panel.
>
> Consecutive quoted blocks read as one region.
>
> > Nesting adds a second bar and deeper indent.

- An unordered item
- Another, with **bold** and `code` inside
  - A nested item one step deeper
    - And a third level

1. First ordered item
2. Second
3. Third, so the marker column is visibly right-aligned

- [x] A checked task
- [ ] An unchecked task

---

The line above is a horizontal rule spanning the content width.

## Tables

|Feature|Status|Notes|
|---|---|---|
|Headings|done|six independent hues|
|Code|done|syntect, pure Rust|
|Tables|new|header row, alternating stripes, and cells that wrap when one of them carries a longer sentence like this one|
|`inline code`|works|styles apply inside cells with **bold** too|

A table with short content stays compact instead of stretching:

|Key|Value|
|---|---|
|a|1|
|b|2|

## Images

The oryx mark, rendered inline at its natural size:

![The oryx mark](oryx-test.png)

Vector images rasterize at their intrinsic size, the logo as SVG:

![The mark as SVG](../../assets/icon/oryx.svg)

A broken path renders as a bordered placeholder with the alt text:

![this image does not exist](missing-image.png)

### Heading Levels

The copper ramp encodes hierarchy: the larger the heading, the warmer and
brighter its color.

#### Level Four Recedes

##### Level Five Is Bold At Body Size

###### Level Six Is The Quietest

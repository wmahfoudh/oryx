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

### Heading Levels

The copper ramp encodes hierarchy: the larger the heading, the warmer and
brighter its color.

#### Level Four Recedes

##### Level Five Is Bold At Body Size

###### Level Six Is The Quietest

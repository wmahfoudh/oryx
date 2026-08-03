---
title: Oryx Feature Tour
status: living document
updated: continuously
---

# Oryx Feature Tour

This document exercises every rendering feature as it lands. Open it after
each task and the newest section shows what just became visible. Emoji
shortcodes render through font fallback: :tada: :rocket: :sparkles:

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

## Alerts

> [!NOTE]
> Useful information a reader should notice even when skimming.

> [!TIP]
> A helpful suggestion for doing something better.

> [!IMPORTANT]
> Key information required to achieve a goal.

> [!WARNING]
> Urgent attention needed to avoid problems.

> [!CAUTION]
> Consequences ahead; this one spans two blocks.
>
> The panel and colored bar cover both paragraphs as one region.

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

Remote images fetch in the background and cache on disk; badges are SVG:

![build badge](https://img.shields.io/badge/build-passing-brightgreen)
![version badge](https://img.shields.io/badge/version-0.3.0-blue)
![license badge](https://img.shields.io/badge/license-GPL--3.0-orange)

A remote raster image arrives the same way, replacing its placeholder:

![remote logo](https://codeberg.org/assets/img/logo.png)

An unreachable remote source keeps the alt placeholder:

![unreachable badge](https://nonexistent.invalid/badge.svg)

## Embedded HTML

<p align="center">
<a href="https://codeberg.org/wmahfoudh/oryx"><img src="https://img.shields.io/badge/oryx-fast-brightgreen" height="20"></a>
<img src="https://img.shields.io/badge/html-subset-blue" height="20">
</p>

Inline styling: <b>bold</b>, <i>italic</i>, H<sub>2</sub>O, x<sup>2</sup>,
and <kbd>Ctrl</kbd>+<kbd>T</kbd>. A break<br>via the br tag.

## Footnotes and Math

Footnote references[^1] render superscript in the link color and click-jump
to their definition; here is a second one[^note] with a word label. The
definitions collect at the end of the document under a rule, wherever they
were written.

Inline math typesets in STIX with real scripts: $E=mc^2$,
$a_i + b^{10}$, and $x_{max}$ flow with the sentence. Block math
centers on its own line:

$$x_n^2 + y_n^2 = z_n^2$$

Currency stays prose: $5-$10 and US$100 vs CA$120 never become
equations, while $`k^2`$ forces math through the backtick form.
Constructs typeset by Appendix G's rules, bar on the axis, surd
stretched, limits stacked in display:

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

[^1]: The first footnote definition, written mid-document.
[^note]: The second definition, with the label rendered as its marker.

## Links

External links open in the system browser: the
[Oryx repository](https://codeberg.org/wmahfoudh/oryx) lives on Codeberg.
Bare autolinks work too: https://example.com.

Anchor links jump inside the document: back to [Text Styles](#text-styles),
or down to [Heading Levels](#heading-levels). The cursor turns into a
pointer over any link.

A link to another file opens it in place:
[the outline sample](samples/sample-outline.md), or straight to
[a section of it](samples/sample-outline.md#behavior).

### Heading Levels

The copper ramp encodes hierarchy: the larger the heading, the warmer and
brighter its color.

#### Level Four Recedes

##### Level Five Is Bold At Body Size

###### Level Six Is The Quietest

## Embedded HTML Tables

A table written in HTML renders through the same grid as a markdown one.
With a `<thead>`, the header row is bold on its own band:

<table>
<thead><tr><th>Component</th><th>Role</th></tr></thead>
<tbody>
<tr><td>parser</td><td>maps events to blocks</td></tr>
<tr><td>layout</td><td>places runs and rects</td></tr>
<tr><td>paint</td><td>rasterizes the band</td></tr>
</tbody>
</table>

<table>
<caption>A headerless table keeps its caption above it</caption>
<tr><td>no thead</td><td>no th cells</td></tr>
<tr><td>so no band</td><td>stripes from the top</td></tr>
</table>

## Details and Summary

<details>
<summary>Click to expand this section</summary>

Hidden content renders only while the section is open. Markdown works
inside: **bold**, `code`, and lists.

- one
- two

</details>

<details open>
<summary>This one starts open</summary>

The `open` attribute in the source sets the initial state.

<details>
<summary>Sections nest</summary>

An inner section folds independently of its parent.

</details>
</details>

## HTML Long Tail

<h3>An HTML heading with an anchor</h3>

<blockquote>An HTML blockquote sits on the quote machinery.</blockquote>

<ul>
<li>an unordered item<ul><li>nested one step</li></ul></li>
<li>a second item</li>
</ul>

<ol start="7">
<li>numbering honors the start attribute</li>
<li>and counts on</li>
</ol>

<pre><code class="language-rust">fn html_pre() -> &'static str {
    "highlighted like a fence"
}</code></pre>

<dl>
<dt>Definition term</dt>
<dd>Its body, indented one step with no marker.</dd>
</dl>

<p><u>Underlined</u>, <mark>highlighted</mark>, <small>small print</small>,
<q>quoted</q>, <cite>a citation</cite>, <samp>program output</samp>.</p>

<hr>

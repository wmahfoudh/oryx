# Oryx syntax reference

Every construct Oryx recognizes, markdown and embedded HTML, shown as
source. Each sample sits in a code block, so this file reads the same
everywhere, including in Oryx itself. Copy any sample into a document to
see it rendered. For math and alerts, where the result is the point,
the rendered form also appears right under its source.

## Headings

```markdown
# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

Setext Heading 1
================

Setext Heading 2
----------------
```

Every heading gets an anchor from its text, in the GitHub style, and
joins the sidebar outline. A heading repeated later in the file gets a
numbered anchor, `-1` for the second, `-2` for the third, as on GitHub.

## Inline styles

```markdown
**bold**  *italic*  ***bold italic***  ~~strikethrough~~  `inline code`

Smart punctuation: "quotes", 'quotes', dashes -- and ---, ellipsis...

Emoji shortcodes: :tada: :rocket: :warning:
```

## Lists

```markdown
- unordered item
* also unordered
+ also unordered
  - nested one level
    - nested two levels

1. ordered item
2. ordered item

- [ ] open task
- [x] done task
```

## Blockquotes and alerts

```markdown
> a quote
> > nested one level

> [!NOTE]
> The five GitHub alert kinds render with their own color and title.

> [!TIP]
> [!IMPORTANT]
> [!WARNING]
> [!CAUTION]
```

The first two, rendered:

> [!NOTE]
> A note renders with its own color and title.

> [!TIP]
> A tip renders in its own color too.

## Code

````markdown
```rust
fn fenced() -> &'static str {
    "highlighted when the language is recognized"
}
```

~~~python
def tilde_fences():
    return "work the same"
~~~

    an indented block renders as code too
````

Fence languages cover the bundled grammar collection, from `rust` and
`python` through `toml`, `kotlin`, `swift`, `typescript`, `dockerfile`,
`zig`, `terraform`, `graphql` and `protobuf`. Oryx also opens source
files directly and renders the whole file highlighted.

## Tables

```markdown
| Left | Center | Right |
|:-----|:------:|------:|
| a    | b      | c     |
| long cells wrap | stripes alternate | columns size to content |
```

## Links and images

```markdown
[a link](https://example.com)
[a section link](#headings)
[a file link](README.md)
[a file link to a section](docs/guide.md#tables)
Bare URLs autolink: https://example.com
[a reference link][ref]

[ref]: https://example.com

![local image](images/logo.png)
![remote image](https://example.com/badge.svg)
```

Remote images fetch in the background and cache on disk. SVG renders,
badges included. A broken path becomes a placeholder with the alt text.

## Footnotes

```markdown
A claim with a footnote.[^1]

[^1]: The definition gathers at the foot of the document.
```

## Math

````markdown
Inline math: $e^{i\pi} + 1 = 0$, or fenced: $`a^2 + b^2 = c^2`$

$$
\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}
$$

```math
\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}
```
````

The same samples, rendered:

Inline math: $e^{i\pi} + 1 = 0$, or fenced: $`a^2 + b^2 = c^2`$

$$
\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}
$$

```math
\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}
```

All four GitHub notations typeset through the same TeX engine in STIX
Two Math. Oryx infers whether a dollar sign is a currency or a math
delimiter: a digit right after a closing dollar, as in `$5-$10`, keeps
it text.

```latex
x_i^2 \quad \frac{a}{b} \quad \binom{n}{k} \quad \sqrt[3]{x^3+y^3}
\left( \frac{a}{b} \right)^2 \quad \sum_{n=1}^{\infty} \quad \oint_0^1
\hat{x} \quad \widehat{abc} \quad \vec{v} \quad \overrightarrow{AB}
\mathbb{R} \quad \mathbf{v} \quad \mathcal{L} \quad \mathfrak{g}
\text{if } \quad \mathrm{d}x \quad \operatorname{Var}(X) \quad \lim_{x \to 0}
\alpha \quad \Omega \quad \hbar \quad \forall \quad \nleq \quad \hookrightarrow
a\,b \quad c\;d \quad e\!f \quad g \qquad h
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}
\begin{aligned} x &= y \\ z &= w \end{aligned}
\newcommand{\avg}[1]{\left\langle #1 \right\rangle} \avg{x^2}
```

A few of these, rendered:

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix} \quad \sqrt[3]{x^3+y^3} \quad \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases} \quad \mathbb{R} \quad \hat{x}
$$

The command vocabulary follows KaTeX's: Greek letters with their
variants, binary operators, relations and their negations, arrows, big
operators, delimiters, the seven math alphabets, accents, operator
names, spacing, and the environments `matrix`, `pmatrix`, `bmatrix`,
`Bmatrix`, `vmatrix`, `Vmatrix`, `smallmatrix`, `cases`, `aligned` and
`array`. `\newcommand` and `\renewcommand` define macros with up to
nine parameters and one optional default. Anything the engine does not
recognize renders as its literal source in place, and runaway macro
definitions degrade the same way.

## Frontmatter

```markdown
---
title: Document title
tags: notes
---
```

A YAML header renders as a metadata panel above the document.

## Horizontal rules

```markdown
---
***
___
```

## Embedded HTML

Oryx renders the HTML subset GitHub allows in READMEs. Anything outside
it is stripped, keeping the inner text.

### Structure

```html
<h2>An HTML heading, with an anchor like any other</h2>

<p align="center">A centered paragraph.</p>
<div align="center">A centered block.</div>

<blockquote>A quote, nestable, stacking with markdown quotes.</blockquote>

<ul><li>bullets<ul><li>nested</li></ul></li></ul>
<ol start="7"><li>ordered, honoring start</li></ol>

<pre><code class="language-rust">fn pre_blocks() {}</code></pre>

<dl>
  <dt>A term, rendered bold</dt>
  <dd>Its definition, indented.</dd>
</dl>

<hr>
```

### Tables

```html
<table>
  <thead><tr><th>With thead</th><th>Header band</th></tr></thead>
  <tbody><tr><td>body</td><td>rows</td></tr></tbody>
</table>

<table>
  <tr><th>A leading th row</th><th>is the header too</th></tr>
  <tr><td>a</td><td>b</td></tr>
</table>

<table>
  <caption>A caption renders centered above</caption>
  <tr><td>no header at all</td><td>no header band</td></tr>
</table>

<table>
  <tr><td><img src="badge.svg" width="90"> images work in cells</td></tr>
</table>
```

`colspan`, `rowspan` and `align` attributes are ignored; each cell takes
one grid slot.

### Collapsible sections

```html
<details>
<summary>Closed by default, click to open</summary>

Markdown works inside. Search still finds this text, and stepping to a
match reveals the section.

</details>

<details open>
<summary>The open attribute starts it expanded</summary>

<details><summary>Sections nest</summary></details>
</details>
```

### Inline

```html
<b>bold</b> <strong>strong</strong> <i>italic</i> <em>emphasis</em>
<code>code</code> <kbd>Ctrl</kbd> <samp>output</samp> <tt>teletype</tt>
<u>underline</u> <ins>inserted</ins> <s>struck</s> <del>deleted</del>
<strike>struck</strike> <mark>highlighted</mark> <small>small</small>
<q>quoted</q> <cite>cited</cite> <dfn>defined</dfn> <var>variable</var>
x<sup>2</sup> H<sub>2</sub>O line<br>break

<a href="https://example.com"><img src="badge.svg" alt="a clickable badge"></a>
<img src="logo.png" width="120" height="40">

<picture>
  <source srcset="ignored.webp">
  <img src="used.png" alt="picture reduces to its img">
</picture>
```

### Entities

```html
&lt; &gt; &amp; &quot; &#39; &nbsp; &copy; &mdash; &eacute; &#169; &#x1F600;
```

Entities decode everywhere in HTML text, `<pre>` included: the five basic
ones, the Latin-1 set from `&nbsp;` to `&yuml;` with the accented letters,
the common typographic names (`&mdash;`, `&ndash;`, `&hellip;`, `&ldquo;`,
`&rdquo;`, `&bull;`, `&euro;`, `&trade;`, the arrows), and numeric
references in decimal or hex. Anything else stays as typed.

## Useful searches

`Ctrl+F` searches any document, and the `.*` button in the search bar
(or `Alt+R`) switches to regular expressions, in the Rust `fancy-regex`
flavor. Some searches worth keeping around:

| Pattern | Finds |
|---|---|
| `TODO\|FIXME` | task markers left in a file |
| `\bhttps?://\S+` | web links written out |
| `\d{4}-\d{2}-\d{2}` | dates like 2026-08-16 |
| `\b(\w+) \1\b` | the same word typed twice in a row |
| `"[^"]*"` | anything between double quotes |
| `^#+ ` | heading lines, in a markdown source |
| ` +$` | spaces left at the end of a line |

In the editor, `Ctrl+H` adds a replace field, and a replacement can
reuse captured groups:

| Search | Replace | Result |
|---|---|---|
| `(\w+)/(\w+)` | `$2/$1` | swaps the two sides of every pair |
| ` +$` | nothing | strips trailing spaces; `Ctrl+Enter` does the whole file |
| `- \[ \]` | `- [x]` | ticks every open task |

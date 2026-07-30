# Oryx syntax reference

Every construct Oryx recognizes, markdown and embedded HTML, shown as
source. Each sample sits in a code block, so this file reads the same
everywhere, including in Oryx itself. Copy any sample into a document to
see it rendered.

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
joins the sidebar outline.

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

```markdown
Inline math: $e^{i\pi} + 1 = 0$

$$
\sum_{n=1}^{\infty} \frac{1}{n^2}
$$
```

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
&lt; &gt; &amp; &quot; &#39;
```

The five basic entities decode everywhere in HTML text, verbatim inside
`<pre>`.

# The extended syntax {#top}

A field fixture: every construct of the Markdown Guide's test file, in our own words. Open it in Oryx and every section below should read in its rendered form, none as raw markup.

## Headings with an id {#custom-heading}

This heading carries `{#custom-heading}` in its source. The braces stay out of the title, and [a link to it](#custom-heading) works, as does [one to the top](#top).

## Definition lists

Oryx
: A fast viewer and editor for markdown, code and books.

Markdown
: A plain text format that reads well as written.
: Also the name of the tool that first converted it.

A term with two paragraphs
: The first paragraph of the definition.

    The second paragraph, indented under the same term.

After the list, an ordinary paragraph.

## Subscript and superscript

Water is H~2~O and carbon dioxide is CO~2~. Energy is E = mc^2^, and the area of a square is a^2^. A tilde between spaces ~still strikes through~ as on GitHub, and ~~two tildes~~ always do.

## Highlight

Two equal signs ==light up a phrase== and so do ::two colons::. Code such as `std::vector` is left alone, and so is std::vector in prose, or a == b in a sentence.

## Abbreviations

The HTML specification is maintained by the W3C. HTML5 is a different word and stays plain. Hover HTML or W3C to read the expansion.

*[HTML]: Hyper Text Markup Language
*[W3C]: World Wide Web Consortium

## Footnotes in order of use

The second label defined comes first in the text,[^second] then the first,[^first] and the second once more.[^second] The markers show 1 and 2 in that order, whatever the labels say.

[^first]: Defined first, used second, so it shows as 2.

[^second]: Defined second, used first, so it shows as 1.

    A second paragraph of the same note, indented under its number.

    `code` in a third paragraph.

## The standard set, for comparison

Paragraphs, **bold**, *italic*, ***both***, ~~struck~~, `code`, a [link](https://example.com), a bare one https://example.com/~user/page, an autolink <https://example.com> and a mail <someone@example.com>.

A line ending in two spaces  
breaks here. A line ending in a backslash\
breaks too.

> A quote
>
> > nested once

1. One
2. Two
    1. Two point one
3. Three

- Dash
- Dash again
  - Nested
- [x] A done task
- [ ] An open task

```json
{ "key": "value", "count": 3 }
```

    An indented code block.

| Column | Other |
| --- | --- |
| a | b |

Emoji pasted ☕ and by shortcode :tada:.

<em>HTML italic</em> and <strong>HTML bold</strong> inline.

---

The end.

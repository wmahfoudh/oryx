# This is Oryx searching in a document

Press Ctrl+F and type `oryx` to see this page fill with highlights. Every
match takes the theme's match color, the current one takes a stronger color,
and Enter or F3 walks through them with a wrapping counter in the bar.

Matching is smart case. A query in lowercase, `oryx`, matches Oryx, ORYX and
oryx alike. Add a capital, `Oryx`, and the match becomes exact, so ORYX and
oryx drop out of the results while Oryx stays.

## Matches in every kind of content

A match is found wherever text is laid out, not only in paragraphs. This
paragraph mentions oryx twice: once here, and once as Oryx at the end.

- A list item naming Oryx
- Another item, mentioning oryx in lowercase
- [A link whose text says Oryx](https://codeberg.org/wmahfoudh/oryx)

| Where | Text |
|---|---|
| A table cell | Oryx |
| Another cell | oryx again |

> A blockquote that mentions Oryx, because quoted text is laid out like any
> other text and is searched the same way.

```rust
// Even inside a code block: oryx
let viewer = "Oryx";
```

## Matching across styles

A match spans styled runs, so searching for `fast viewer` finds it even
though it is written as **fast** *viewer* with the two words in different
styles. What it will not do is match across a block boundary: the end of
this paragraph and the start of the next are separate.

Oryx keeps your place while you search. Zoom in mid-search, or reload the
file, and the query re-runs against the new layout with the current match
kept in view.

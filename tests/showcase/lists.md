# This is Oryx rendering lists and task lists

Unordered lists take a bullet sized to the text, and nesting indents by a
fixed step so the levels line up down the page.

- A first item, at the top level
- A second item, long enough to wrap onto a second line so the continuation
  aligns under the text rather than under the bullet
  - A nested item, one level in
  - Another nested item
    - A third level, indenting again
- Back at the top level

Ordered lists number themselves from the source, and the marker is laid out
as its own run so the text after it aligns regardless of the number width.

1. The first step
2. The second step
3. The third step, which wraps the same way an unordered item does and keeps
   its continuation aligned with the text
   1. A nested ordered item
   2. And a second one
10. A two-digit marker, still aligned

Task lists draw real checkboxes, filled when checked, in the theme's colors.

- [x] Render the document natively
- [x] Draw every pixel on the CPU
- [x] Ship as one small binary
- [ ] Embed a browser engine
- [ ] Require a runtime

Lists sit tighter against each other than paragraphs do: consecutive items
take a small gap, while the space before and after the whole list matches
the surrounding block spacing.

- Tight against the item above
- And the one below

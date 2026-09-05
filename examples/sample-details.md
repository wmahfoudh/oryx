# Collapsible sections

Collapsible sections, the way GitHub renders them: a section starts
closed, and clicking the summary row opens it.

<details>
<summary>Installation notes</summary>

Nothing here is visible until the row is clicked. Search still finds
this text, and stepping to a match reveals it: try finding the word
crocodile.

- step one
- step two

</details>

<details open>
<summary>Starts open via the open attribute</summary>

### A heading inside

This heading has an anchor and joins search and select-all like any
other block: [jump to it](#a-heading-inside) from anywhere.

<details>
<summary>Nested fold</summary>

Inner sections fold on their own. Select-all copies this text whether
the fold is open or closed.

</details>
</details>

<details>

A details block without a summary gets a default "Details" title.

</details>

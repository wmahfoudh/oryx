# This is Oryx rendering tables

Columns size themselves to their content up to a cap, rows alternate their
background, and the header spans in bold above a grid drawn in the theme's
own colors.

| Shortcut | Action | Notes |
|---|---|---|
| Ctrl+O | Open file | Native dialog, filtered to what Oryx reads |
| Ctrl+T | Theme browser | Previews and applies live |
| Ctrl+B | Folder sidebar | Keyboard navigable |
| Ctrl+F | Find in document | Smart case matching |
| F1 | Shortcuts help | Every binding, in one screen |

Alignment markers in the separator row are honored per column, left, center
and right:

| Language | Extension | Files |
|:---|:---:|---:|
| Rust | `.rs` | 1284 |
| Python | `.py` | 96 |
| TypeScript | `.ts` | 12 |
| Markdown | `.md` | 7 |

Cells carry inline styling of their own, and long cells wrap inside their
column instead of pushing the table off the page:

| Element | Behavior |
|---|---|
| **Bold cell** | Styling inside a cell renders exactly as it does in a paragraph |
| `code cell` | Inline code keeps its pill and its monospace family |
| [Link cell](https://codeberg.org/wmahfoudh/oryx) | Links stay clickable inside the grid |
| Wrapping cell | A deliberately long cell, written to run past the column width so the wrapping inside the cell is visible and the row grows to fit it rather than overflowing |

A compact table stays narrow rather than stretching to the full content
width:

| Yes | No |
|---|---|
| 1 | 0 |

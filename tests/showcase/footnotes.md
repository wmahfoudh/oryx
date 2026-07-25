# This is Oryx rendering footnotes

Footnote references render as raised superscript markers in the flow of the
sentence[^why], sized down from the body text so they mark without
interrupting. Clicking one jumps to its definition.

Definitions collect at the end of the document under a rule, in reference
order, wherever they were written in the source[^order]. The one below was
written first in the file and still lands in its proper place.

A paragraph can carry several notes at once[^one] without the line spacing
shifting around them[^two], because a raised marker is laid out inside the
line box rather than above it[^three].

Notes can be as long as they need to be, wrapping across several lines at a
slightly smaller size than the body so the block reads as apparatus rather
than as prose[^long].

[^order]: Written first in the source, placed third here, because
    definitions are ordered by the references that point at them.

[^why]: The jump is a real anchor: Oryx resolves it against the laid-out
    document, so it lands exactly on the definition.

[^one]: The first of three in one paragraph.

[^two]: The second. Markers stay on their own baseline.

[^three]: The third, which proves the line spacing above it did not move.

[^long]: A longer note, to show how a definition wraps. It runs past a
    single line so the indentation and the reduced size are both visible,
    and it ends without any of the surrounding text being pushed around.

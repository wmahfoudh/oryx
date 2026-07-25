# This is Oryx rendering alerts and blockquotes

GitHub alerts take a titled panel with a colored bar and a matching title,
one color per kind, all drawn from the active theme.

> [!NOTE]
> Useful information that a reader should notice even when skimming.

> [!TIP]
> An optional suggestion that makes something easier.

> [!IMPORTANT]
> Information a reader needs in order to succeed at the task.

> [!WARNING]
> Something that needs immediate attention because of the risk it carries.

> [!CAUTION]
> The consequences of an action that cannot easily be undone.

## Plain blockquotes

A quote without a marker takes the neutral panel and bar:

> The best way to predict the future is to invent it. A quote can run to
> several lines, and the panel closes around all of them as one region
> rather than boxing each line separately.

Quotes nest, and each level takes its own indent and its own bar, so the
depth is readable at a glance:

> A first level quote, introducing what follows.
>
> > A second level, quoted inside the first.
> >
> > > And a third level, as deep as most documents ever go.

Quoted content keeps its own formatting, including **bold**, `code`, and
lists:

> Inside a quote you still get:
>
> - list items with their bullets
> - `inline code` in its pill
> - [links](https://codeberg.org/wmahfoudh/oryx) that stay clickable

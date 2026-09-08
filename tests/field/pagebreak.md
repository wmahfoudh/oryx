# Page breaks

A first stretch of text. The dashed line below marks where the PDF starts a new page.

<div style="page-break-after: always"></div>

## Second page

The pandoc spelling works too, alone on its line.

\newpage

## Third page

Two breaks in a row make one new page, and a break at the very end adds no empty page.

\pagebreak

\clearpage

## Fourth page

The last stretch.

<div style="page-break-before: always"></div>

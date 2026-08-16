# This is Oryx rendering a GitHub style README

Real project READMEs are not plain markdown. They open with a centered logo,
a wrapped row of badges, and a tagline built from raw HTML, because markdown
alone cannot center anything. Oryx renders that subset directly, so a README
looks the way its author intended.

<p align="center">
<img src="../../assets/icon/oryx.svg" height="96">
</p>

<p align="center">
<b>A fast, native markdown code, text viewer and editor</b><br>
Ebook Reader, PDF export<br>
One binary, no runtime
</p>

## What the HTML subset covers

Centered blocks through `<p align="center">` and `<div align="center">`,
images with an explicit `width` or `height`, images wrapped in links so a
badge stays clickable, `<br>` as a hard line break, and the inline styling
tags: <b>bold</b>, <i>italic</i>, <code>code</code>, H<sub>2</sub>O with a
subscript and 10<sup>6</sup> with a superscript.

<p align="center">
<img src="https://img.shields.io/badge/this_is-Oryx-orange" height="25">
<img src="https://img.shields.io/badge/rendering-a_GitHub_README-blue" height="25">
<img src="https://img.shields.io/badge/drawn_on_the-CPU-red" height="25">
<img src="https://img.shields.io/badge/startup-instant-brightgreen" height="25">
</p>
<p align="center">
<img src="https://img.shields.io/badge/this_is-Oryx-orange" height="20">
<img src="https://img.shields.io/badge/rendering-a_GitHub_README-blue" height="20">
<img src="https://img.shields.io/badge/drawn_on_the-CPU-black" height="20">
<img src="https://img.shields.io/badge/startup-instant-brightgreen" height="20">
</p>

# This is Oryx rendering a GitHub style README

Real project READMEs are not plain markdown. They open with a centered logo,
a wrapped row of badges, and a tagline built from raw HTML, because markdown
alone cannot center anything. Oryx renders that subset directly, so a README
looks the way its author intended.

<p align="center">
<img src="../../assets/icon/oryx.svg" height="96">
</p>

<p align="center">
<b>A fast, native markdown viewer</b><br>
Reading, not editing<br>
One binary, no runtime
</p>

## What the HTML subset covers

Centered blocks through `<p align="center">` and `<div align="center">`,
images with an explicit `width` or `height`, images wrapped in links so a
badge stays clickable, `<br>` as a hard line break, and the inline styling
tags: <b>bold</b>, <i>italic</i>, <code>code</code>, H<sub>2</sub>O with a
subscript and 10<sup>6</sup> with a superscript.

<p align="center">
<a href="https://codeberg.org/wmahfoudh/oryx"><img src="https://img.shields.io/badge/source-Codeberg-blue" height="20"></a>
<a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/built_in-Rust-black" height="20"></a>
<a href="https://www.gnu.org/licenses/gpl-3.0"><img src="https://img.shields.io/badge/license-GPL--3.0-orange" height="20"></a>
</p>

Badges also sit inline inside ordinary text, at the size the tag asks for:
the project is <img src="https://img.shields.io/badge/status-stable-brightgreen" height="18"> and
runs on <img src="https://img.shields.io/badge/Linux-%7C%20Windows-blue" height="18">.

## What it does not cover

HTML tables and `<details>` sections are not rendered. Any other tag is
stripped and its inner text kept, so an unsupported tag costs its styling
and never its content.

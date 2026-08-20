# This is Oryx hiding what a browser hides

<!-- TOC -->
<!-- This comment block sits between the title and the first paragraph. -->

A README carries text the reader is never meant to see: a `<!-- TOC -->`
marker for a generator, a badge commented out while its service is down,
notes to the next maintainer. GitHub hides all of it. This page exercises
every form Oryx drops, so nothing below this paragraph should show a comment.

## Comments around text

Before the comment <!-- an inline note --> after the comment, on one space.
A comment can hold a closing bracket <!-- a > b, still inside --> and the text
continues. A comment<!-- glued -->between two words leaves no space at all.

<!--
A comment spanning several lines,
with a blank line inside:

and markdown that must not render: **bold**, [a link](https://x.tld).
-->

## A commented-out badge

The row below has three badges in the source and shows two.

<p align="center">
<img src="https://img.shields.io/badge/first-shown-brightgreen" height="25">
<!-- <img src="https://img.shields.io/badge/second-hidden-red" height="25"> -->
<img src="https://img.shields.io/badge/third-shown-blue" height="25">
</p>

## The other invisible forms

A doctype, a processing instruction and a CDATA section, each on its own
line in the source, all hidden:

<!DOCTYPE html>
<?xml version="1.0" encoding="UTF-8"?>
<![CDATA[ raw text with <b>tags</b> that a browser would not show ]]>

## What stays text

A bare `<` before a space or a digit is not a tag: 3 < 4, and x<y, and
a heart <3. A comment that never closes hides the rest of the file, as it
does on GitHub; this page keeps its comments closed so that everything
after them shows. <!-- Every section above is a visible test. -->

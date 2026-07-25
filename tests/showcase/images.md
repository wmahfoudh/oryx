# This is Oryx rendering images

Local raster images are decoded and scaled to the content width, keeping
their aspect ratio, and an image smaller than the text column keeps its
natural size instead of being stretched.

![A local PNG, scaled to fit the column](../fixtures/oryx-test.png)

SVG images are rasterized at their intrinsic size when the document opens,
so vector art stays crisp instead of being blown up from a bitmap.

![The Oryx mark, drawn from SVG](../../assets/icon/oryx.svg)

## Remote images

Images with an http address are fetched in the background and cached on
disk. The document renders immediately with placeholders, and each image
lands as it arrives. On the second open they come from the cache, so a
document full of badges renders instantly, and offline.

![Build](https://img.shields.io/badge/build-passing-brightgreen)
![Version](https://img.shields.io/badge/version-0.7.0-blue)
![License](https://img.shields.io/badge/license-GPL--3.0-orange)

## Missing images

A path that does not resolve becomes a placeholder box carrying the alt
text, so a broken link is visible and identifiable rather than silently
absent:

![This image does not exist and shows its alt text instead](no-such-file.png)

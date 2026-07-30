# The HTML long tail

Everything GitHub's sanitizer allows, rendered through the model.

<h2>Headings join the outline machinery</h2>

Link to one from markdown: [jump to the heading](#headings-join-the-outline-machinery).

<blockquote>
HTML quotes stack with markdown quotes and draw the same bar.
<blockquote>Nested one level deeper.</blockquote>
</blockquote>

<ul>
<li>bullets<ul><li>nest</li><li>deeply</li></ul></li>
<li>as expected</li>
</ul>

<ol start="40">
<li>ordered lists honor start</li>
<li>and keep counting</li>
</ol>

<pre><code class="language-python">def pre_blocks():
    return "lazy-highlighted like any fence"
</code></pre>

<dl>
<dt>Oryx</dt>
<dd>A fast, native viewer for markdown and code.</dd>
<dt>Grammar</dt>
<dd>A syntax definition compiled into the binary at build time.</dd>
</dl>

Inline: <u>underline</u>, <ins>inserted</ins>, <s>struck</s>,
<mark>marked</mark>, <small>small</small>, <q>quoted</q>,
<cite>cited</cite>, <var>variable</var>, <samp>sample</samp>,
<kbd>Ctrl</kbd>+<kbd>F</kbd>, x<sup>2</sup>, H<sub>2</sub>O.

<picture>
<source srcset="does-not-matter.webp">
<img src="../oryx-test.png" alt="the picture element reduces to its img">
</picture>

<hr>

An `<input type="checkbox">` degrades to its text, and unknown tags
keep stripping to their inner text.

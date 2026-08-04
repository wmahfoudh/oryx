# HTML tables

READMEs use raw HTML tables when they need images in cells or want to
skip the header. They render with the same grid, stripes and rounding
as markdown tables.

## With a thead

<table>
<thead><tr><th>Tier</th><th>Open</th><th>Layout</th></tr></thead>
<tbody>
<tr><td>64KB</td><td>40ms</td><td>14ms</td></tr>
<tr><td>1MB</td><td>40ms</td><td>244ms</td></tr>
<tr><td>8MB</td><td>40ms</td><td>2349ms</td></tr>
</tbody>
</table>

## Leading th row, no thead

<table>
<tr><th>Key</th><th>Value</th></tr>
<tr><td>theme</td><td>oryx-dark</td></tr>
<tr><td>zoom</td><td>1.0</td></tr>
</table>

## Headerless

No `<thead>`, no `<th>`: no header band, stripes count from the top.

<table>
<tr><td>alpha</td><td>beta</td><td>gamma</td></tr>
<tr><td>delta</td><td>epsilon</td><td>zeta</td></tr>
<tr><td>eta</td><td>theta</td><td>iota</td></tr>
</table>

## Images in cells

<table>
<tr>
<td><img src="oryx-test.png" width="48"> logo left</td>
<td>plain text cell</td>
</tr>
<tr>
<td>plain text cell</td>
<td><img src="oryx-test.png" width="48"> logo right</td>
</tr>
</table>

## Caption and colspan

The caption renders centered above the table. A `colspan` cell takes one
grid slot; spanning is not supported.

<table>
<caption>Release sizes per platform</caption>
<tr><th>Platform</th><th>Size</th></tr>
<tr><td>Linux</td><td>16MB</td></tr>
<tr><td colspan="2">Windows build pending</td></tr>
</table>

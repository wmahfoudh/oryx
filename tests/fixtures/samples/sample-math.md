# Rendered math

## Inline

Equations sit inside their sentences: the identity $E = mc^2$, the
sequence $a_i^2 + b_i^2$, and Euler's $e^{i\pi} + 1 = 0$ keep the text
baseline. Scripts stack on one atom as in $x_i^2$ and brace into groups
as in $b^{10}$ or $x_{max}$.

## Display

Dollar delimiters center an equation on its own line:

$$x_n^2 + y_n^2 = z_n^2$$

GitHub's fence renders the same way:

```math
\alpha^2 + \beta^2 = \gamma^2
```

## Symbols

Greek reads italic in lowercase, upright in capitals:
$\alpha \beta \gamma \delta \pi \sigma \omega$ beside
$\Gamma \Delta \Sigma \Omega$. Relations space themselves:
$a \leq b \neq c \approx d \equiv e$. Binary operators sit tighter:
$x \pm y \times z \cdot w$. The big symbols exist ahead of their limit
machinery: $\sum$, $\prod$, $\int$, and the singletons $\infty$,
$\nabla$, $\partial$.

## The gate

Prices never become equations: $5-$10, US$100 vs CA$120, and
prices like $5, $6, and $7 all stay prose. The backtick form forces
math where ambiguity remains: $`k^2`$. An escaped \$50 stays a dollar.

## Degradation

A command the engine does not know yet degrades quietly, in place:
$x = \undefinedop{y} + z^2$ typesets everything it understands and
carries the rest as literal TeX in the math color.

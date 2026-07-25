# This is Oryx rendering math

Math is written as TeX literals between dollar signs. Oryx substitutes the
symbols and raises the scripts, so an expression reads as an expression
rather than as source: the mass-energy relation $E = mc^2$, a tolerance of
$\epsilon \leq 0.01$, and a sum $\sum_{i=1}^{n} x_i^2$ all sit inline in the
paragraph at the surrounding text size.

Block math is centered on its own line:

$$
\sum_{i=1}^{n} (x_i - \mu)^2 \leq \sigma^2 \cdot n
$$

$$
\forall \epsilon > 0, \exists \delta > 0 : |x - a| < \delta \Rightarrow |f(x) - f(a)| < \epsilon
$$

$$
\int_{0}^{\infty} e^{-x^2} dx = \sqrt\pi / 2
$$

## Greek letters and operators

Lowercase $\alpha \beta \gamma \delta \epsilon \theta \lambda \mu \pi
\sigma \phi \omega$ and uppercase $\Gamma \Delta \Theta \Lambda \Pi \Sigma
\Phi \Psi \Omega$ come through as their own glyphs.

So do the operators you actually reach for: $a \times b$, $a \cdot b$,
$a \div b$, $x \pm y$, $a \neq b$, $a \approx b$, $a \equiv b$,
$a \propto b$, $\nabla f$, $\partial x$, and $\infty$.

Set and logic notation reads the same way: $x \in A$, $y \notin B$,
$A \subseteq B$, $A \cup B$, $A \cap B$, $A \to B$, $p \wedge q$,
$p \vee q$, $\neg p$, and the empty set $\emptyset$.

## Scripts

Superscripts and subscripts bind the next character or a braced group,
exactly as in TeX: $x^2$, $a_i$, $x^{n+1}$, $a_{i,j}$, and both at once in
$x_i^2$. Nested cases like $\sigma_{max}^{2}$ keep their grouping.

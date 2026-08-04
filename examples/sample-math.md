# Math in Oryx

## Inline

Math between single dollars renders inside the sentence: $E = mc^2$,
$a_i^2 + b_i^2$, and $e^{i\pi} + 1 = 0$ sit on the text baseline.
Superscripts and subscripts use `^` and `_`, as in $x_i^2$; braces
group longer scripts, as in $b^{10}$ or $x_{max}$.

## Display

Double dollars put an equation on its own line, centered:

$$x_n^2 + y_n^2 = z_n^2$$

A math fence does the same:

```math
\alpha^2 + \beta^2 = \gamma^2
```

## Symbols

Lowercase Greek is italic, capitals are upright:
$\alpha \beta \gamma \delta \pi \sigma \omega$ and
$\Gamma \Delta \Sigma \Omega$. Relations: $a \leq b \neq c \approx d
\equiv e$. Binary operators: $x \pm y \times z \cdot w$. Big
operators and common symbols: $\sum$, $\prod$, $\int$, $\infty$,
$\nabla$, $\partial$.

## Fractions, roots, delimiters

The quadratic formula:

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

Limits go above big operators in display equations, and beside
integrals:

$$\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$$

$$\int_0^{\infty} e^{-x^2} dx = \frac{\sqrt{\pi}}{2}$$

Binomials: $\binom{n}{k}$. Roots take an optional degree:
$\sqrt[23]{x+1}$. `\left` and `\right` delimiters grow with their
content:

$$\left( \frac{1}{1 + \frac{1}{x}} \right)^2$$

Inline, the same constructs stay at text size: $\frac{a+b}{2}$,
$\sqrt{2}$, $\sum_i x_i$.

## Accents, alphabets, text

Accents: $\hat x$, $\vec v$, $\bar y$, $\tilde q$, $\dot r$. The wide
forms stretch over the whole argument: $\widehat{abc}$,
$\widetilde{xyz}$. Operator names render upright:

$$\sin^2 \theta + \cos^2 \theta = 1$$

$$\lim_{n \to \infty} \left( 1 + \frac{1}{n} \right)^n = e$$

The alphabet commands: $\mathbb{R}$, $\mathbb{N}$, $\mathbf{v}$,
$\mathcal{L}$, $\mathfrak{g}$, $\mathsf{T}$, $\mathtt{x}$. `\text`
puts normal words inside an equation: $f(x) = 1 \text{ if } x > 0$.
Spacing commands adjust gaps: $a\,b$, $a\;b$, $a\quad b$, and $a\!b$
tightens.

An equation wider than the window shrinks to fit:

$$(a+b)^{10} = a^{10} + 10a^9b + 45a^8b^2 + 120a^7b^3 + 210a^6b^4 + 252a^5b^5 + 210a^4b^6 + 120a^3b^7 + 45a^2b^8 + 10ab^9 + b^{10}$$

## Matrices, cases, aligned

A matrix in parentheses:

$$R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}$$

A piecewise definition:

$$|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}$$

A derivation aligned on its equals signs:

$$\begin{aligned} (a+b)^2 &= (a+b)(a+b) \\ &= a^2 + 2ab + b^2 \end{aligned}$$

Determinants and norms use `vmatrix` and `Vmatrix`:
$\begin{vmatrix} a & b \\ c & d \end{vmatrix}$,
$\begin{Vmatrix} v \end{Vmatrix}$. A `smallmatrix` fits inline:
$\begin{smallmatrix} 1 & 0 \\ 0 & 1 \end{smallmatrix}$. Tall
parentheses are assembled from pieces:

$$\begin{pmatrix} 1 \\ 2 \\ 3 \\ 4 \\ 5 \\ 6 \\ 7 \\ 8 \\ 9 \\ 10 \end{pmatrix}$$

## Macros

`\newcommand` defines a command; it works from its definition to the
end of the same equation:

$$\newcommand{\avg}[1]{\left\langle #1 \right\rangle} \operatorname{Var}(X) = \avg{X^2} - \avg{X}^2$$

An argument in brackets is optional and has a default, here the
norm's index:

$$\newcommand{\norm}[2][2]{\lVert #2 \rVert_{#1}} \norm{v} \quad \norm[1]{v} \quad \norm[\infty]{v}$$

## More symbols

Arrows: $f \colon A \hookrightarrow B \twoheadrightarrow C$,
$x \mapsto x^2$, $P \iff Q \implies R$, $A \rightleftharpoons B$.
Relations and their negations: $a \ll b \preceq c \sim d$,
$A \subsetneq B \nsubseteq C$, $p \nmid q$, $u \parallel v \perp w$.
Big operators: $\bigcup_i A_i \supseteq \bigcap_i A_i$,
$\bigoplus_k V_k$, $\oint_\gamma \omega$, $\iint_D f \, \mathrm{d}A$.

Greek variants and letterlike symbols:
$\varphi\ \vartheta\ \varpi\ \varrho\ \varsigma\ \digamma$, $\hbar$,
$\ell$, $\Re$, $\Im$, $\aleph$, $\wp$, $\mho$. Logic:
$\forall x\ \exists y : \neg(x \land y) \lor \top$. The suits
$\spadesuit \heartsuit \diamondsuit \clubsuit$ and the accidentals
$\flat \natural \sharp$; also $\therefore$ and $\because$.

Paired delimiters: $\lvert x \rvert$, $\lVert A \rVert$,
$\lceil x \rceil$, $\lfloor y \rfloor$, $\ulcorner p \urcorner$.
Arrows stretch as delimiters too:
$\left\uparrow \frac{a}{b} \right\downarrow$. Arrow accents:
$\overrightarrow{AB}$, $\overleftarrow{BA}$,
$\overleftrightarrow{AB}$, plus $\dddot{x}$ and $\widecheck{abc}$.
More operator names: $\operatorname*{argmin}_w f(w)$,
$\operatorname{Var}(X)$, $\csch x$, $\sech y$, $a \bmod n$. The
upright differential: $\frac{\mathrm{d}}{\mathrm{d}x}$.

## Equations from Wikipedia

Real equations, copied as-is from their articles.

Maxwell's equations:

$$\nabla \cdot \vec{E} = \frac{\rho}{\varepsilon_0} \qquad \nabla \cdot \vec{B} = 0$$

$$\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t} \qquad \nabla \times \vec{B} = \mu_0 \vec{J} + \mu_0 \varepsilon_0 \frac{\partial \vec{E}}{\partial t}$$

The Schrödinger equation:

$$i\hbar \frac{\partial}{\partial t} \Psi(\mathbf{r}, t) = \left[ -\frac{\hbar^2}{2m} \nabla^2 + V(\mathbf{r}, t) \right] \Psi(\mathbf{r}, t)$$

The Navier-Stokes momentum equation:

$$\rho \left( \frac{\partial \mathbf{u}}{\partial t} + \mathbf{u} \cdot \nabla \mathbf{u} \right) = -\nabla p + \mu \nabla^2 \mathbf{u} + \rho \mathbf{g}$$

Einstein's field equations:

$$G_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8 \pi G}{c^4} T_{\mu\nu}$$

The Riemann zeta functional equation:

$$\zeta(s) = 2^s \pi^{s-1} \sin\left(\frac{\pi s}{2}\right) \Gamma(1-s) \zeta(1-s)$$

The Fourier transform and its inverse:

$$\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\, e^{-2\pi i x \xi} \,\mathrm{d}x \qquad f(x) = \int_{-\infty}^{\infty} \hat{f}(\xi)\, e^{2\pi i x \xi} \,\mathrm{d}\xi$$

Bayes' theorem:

$$P(A_i \mid B) = \frac{P(B \mid A_i)\, P(A_i)}{\sum_j P(B \mid A_j)\, P(A_j)}$$

## A machine-learning README

The kind of math a project README carries. The model minimizes the
regularized risk

$$\hat{w} = \operatorname*{argmin}_{w \in \mathbb{R}^d} \; \frac{1}{n} \sum_{i=1}^{n} \ell(y_i, \langle w, x_i \rangle) + \lambda \lVert w \rVert_2^2$$

with a convex loss $\ell \colon \mathbb{R} \times \mathbb{R} \to
\mathbb{R}_{\geq 0}$ and $\lambda > 0$. Training uses a decaying step
size $\eta_t \approx 10^{-3}$ and stops when
$\lVert w_{t+1} - w_t \rVert \leq \epsilon$.

```math
\begin{aligned}
w_{t+1} &= w_t - \eta_t \nabla f(w_t) \\
\mathbb{E}[f(w_T)] - f(\hat{w}) &\leq \mathcal{O}\!\left(\frac{1}{\sqrt{T}}\right)
\end{aligned}
```

## Notes with macros

The kind of math a note-taking vault carries. Bra-ket notation,
defined as macros and used in the same equation:

$$\newcommand{\bra}[1]{\left\langle #1 \right\rvert} \newcommand{\ket}[1]{\left\lvert #1 \right\rangle} \newcommand{\ip}[2]{\left\langle #1 \mid #2 \right\rangle} \ip{\psi}{\psi} = 1 \qquad \hat{H} \ket{\psi_n} = E_n \ket{\psi_n}$$

The density matrix $\rho = \sum_n p_n |\psi_n\rangle\langle\psi_n|$
with $\operatorname{tr}(\rho) = 1$, and the Born rule
$p(m) = \operatorname{tr}(M_m^\dagger M_m \rho)$.

A rotation and a piecewise map side by side:

$$R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix} \qquad \operatorname{ReLU}(x) = \begin{cases} x & x > 0 \\ 0 & x \leq 0 \end{cases}$$

## Dollars and prices

Oryx infers whether a dollar sign is a currency or a math delimiter.
Prices like $5-$10, US$100, or "$5, $6, and $7" stay text. To force
math where it guesses wrong, use the backtick form: $`k^2`$. An
escaped \$50 always stays a dollar.

## Unknown commands

A command Oryx does not know renders as literal source, in place, and
the rest of the equation still typesets: $x = \undefinedop{y} + z^2$.
A macro that would expand forever stops at a budget and renders as
literal source too: $\newcommand{\selfref}{\selfref} \selfref$.

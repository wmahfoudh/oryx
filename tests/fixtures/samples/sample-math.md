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

## Constructs

The quadratic formula, the font comparison's own specimen:

$$x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$$

The Basel problem and the Gaussian integral, limits above and beside:

$$\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$$

$$\int_0^{\infty} e^{-x^2} dx = \frac{\sqrt{\pi}}{2}$$

Binomials stack barless in their parentheses: $\binom{n}{k}$ chooses
$k$ from $n$. A degree rides its radical: $\sqrt[23]{x+1}$. Delimiters
grow with what they hold:

$$\left( \frac{1}{1 + \frac{1}{x}} \right)^2$$

Inline, the same machinery stays text-sized: $\frac{a+b}{2}$,
$\sqrt{2}$, and $\sum_i x_i$ sit in the sentence.

## Accents, alphabets, text

Accents place by the font's attachment points: $\hat x$, $\vec v$,
$\bar y$, $\tilde q$, $\dot r$, and the wide forms stretch over their
argument: $\widehat{abc}$, $\widetilde{xyz}$. Operator names set
upright and space as operators, so a trigonometry identity reads as a
book sets it:

$$\sin^2 \theta + \cos^2 \theta = 1$$

$$\lim_{n \to \infty} \left( 1 + \frac{1}{n} \right)^n = e$$

The letter styles map into the mathematical alphabets: $\mathbb{R}$,
$\mathbb{N}$, $\mathbf{v}$, $\mathcal{L}$, $\mathfrak{g}$,
$\mathsf{T}$, $\mathtt{x}$. Words join equations upright through
`\text`: $f(x) = 1 \text{ if } x > 0$. The spacing commands nudge the
pen: $a\,b$, $a\;b$, $a\quad b$, and $a\!b$ tightens.

A deliberately wide equation shrinks uniformly to fit its column:

$$(a+b)^{10} = a^{10} + 10a^9b + 45a^8b^2 + 120a^7b^3 + 210a^6b^4 + 252a^5b^5 + 210a^4b^6 + 120a^3b^7 + 45a^2b^8 + 10ab^9 + b^{10}$$

## Environments

A rotation matrix measures its columns and stretches its parentheses:

$$R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}$$

A piecewise definition sets behind one brace:

$$|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}$$

A derivation lines its relations through aligned:

$$\begin{aligned} (a+b)^2 &= (a+b)(a+b) \\ &= a^2 + 2ab + b^2 \end{aligned}$$

The family covers determinants and norms, $\begin{vmatrix} a & b \\ c & d \end{vmatrix}$
and $\begin{Vmatrix} v \end{Vmatrix}$, and a small matrix rides its sentence:
$\begin{smallmatrix} 1 & 0 \\ 0 & 1 \end{smallmatrix}$ Ten rows assemble their
fences from extenders:

$$\begin{pmatrix} 1 \\ 2 \\ 3 \\ 4 \\ 5 \\ 6 \\ 7 \\ 8 \\ 9 \\ 10 \end{pmatrix}$$

## Macros

A document defines its own commands and uses them in the same span:

$$\newcommand{\avg}[1]{\left\langle #1 \right\rangle} \operatorname{Var}(X) = \avg{X^2} - \avg{X}^2$$

An optional first argument takes a default, here the norm's index:

$$\newcommand{\norm}[2][2]{\lVert #2 \rVert_{#1}} \norm{v} \quad \norm[1]{v} \quad \norm[\infty]{v}$$

## The wider vocabulary

Arrows and mappings: $f \colon A \hookrightarrow B \twoheadrightarrow C$,
$x \mapsto x^2$, $P \iff Q \implies R$, and equilibria
$A \rightleftharpoons B$. Relations with their negations:
$a \ll b \preceq c \sim d$, $A \subsetneq B \nsubseteq C$, $p \nmid q$,
$u \parallel v \perp w$. Big operators range over sets:
$\bigcup_i A_i \supseteq \bigcap_i A_i$, $\bigoplus_k V_k$,
$\oint_\gamma \omega$, $\iint_D f \, \mathrm{d}A$.

Variant Greek and the letterlike singletons:
$\varphi\ \vartheta\ \varpi\ \varrho\ \varsigma\ \digamma$, $\hbar$,
$\ell$, $\Re$, $\Im$, $\aleph$, $\wp$, $\mho$. Logic and ornament:
$\forall x\ \exists y : \neg(x \land y) \lor \top$, the suits
$\spadesuit \heartsuit \diamondsuit \clubsuit$, the accidentals
$\flat \natural \sharp$, and $\therefore$, $\because$.

The delimiter rows pair up: $\lvert x \rvert$, $\lVert A \rVert$,
$\lceil x \rceil$, $\lfloor y \rfloor$, corners
$\ulcorner p \urcorner$, and arrows stretch as fences:
$\left\uparrow \frac{a}{b} \right\downarrow$. The arrow accents cover
vectors both ways: $\overrightarrow{AB}$, $\overleftarrow{BA}$,
$\overleftrightarrow{AB}$, with $\dddot{x}$ and $\widecheck{abc}$
rounding out the family. Operator names grew their analysis set:
$\operatorname*{argmin}_w f(w)$, $\operatorname{Var}(X)$, $\csch x$,
$\sech y$, and $a \bmod n$; the upright differential writes
$\frac{\mathrm{d}}{\mathrm{d}x}$.

## The gate

Prices never become equations: $5-$10, US$100 vs CA$120, and
prices like $5, $6, and $7 all stay prose. The backtick form forces
math where ambiguity remains: $`k^2`$. An escaped \$50 stays a dollar.

## Degradation

A command the engine does not know yet degrades quietly, in place:
$x = \undefinedop{y} + z^2$ typesets everything it understands and
carries the rest as literal TeX in the math color. A runaway macro
exhausts its budget and degrades the same way:
$\newcommand{\selfref}{\selfref} \selfref$ terminates as a literal
instead of recursing.

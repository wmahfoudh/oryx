# Math in Oryx

Oryx typesets the TeX formulas of a markdown file with the STIX Two Math font. This page shows every command it knows. It is written for Oryx: open it there, or press `F1` and follow the Math link. GitHub does not render every command on it.

The page has two parts. The [Lessons](#lessons) go from the first formula to the advanced ones. The [Reference](#reference) lists the commands by family.

## Lessons

Each lesson shows the markdown first, then what Oryx renders from it. The commands are those of TeX. Most of them also work in LaTeX, on Wikipedia and on GitHub.

### Writing a formula

A formula goes between dollar signs. With one dollar on each side, the formula is part of the sentence. Two dollars on each side, or a `math` code block, put it on a line of its own, centered. GitHub's form, `` $`...`$ ``, works too.

````markdown
The area of a disk is $A = \pi r^2$, and $`e^{i\pi} = -1`$ renders the same way.

$$
a^2 + b^2 = c^2
$$

```math
\int_0^1 x^2 \, dx = \frac{1}{3}
```
````

The area of a disk is $A = \pi r^2$, and $`e^{i\pi} = -1`$ renders the same way.

$$
a^2 + b^2 = c^2
$$

```math
\int_0^1 x^2 \, dx = \frac{1}{3}
```

Prices such as `$5-$10` stay text: Oryx does not close a formula at a dollar followed by a digit. Inside a formula, write `\$` for a dollar sign and `\%` for a percent sign. A bare `%` starts a comment until the end of the line, as in TeX. If the first line of a `$$` formula starts with a minus sign and a space, markdown reads it as a list item, on GitHub and in Oryx. Write such a formula on one line.

```markdown
$$- \frac{a}{b} + c$$
```

$$- \frac{a}{b} + c$$

In the examples of this page, `\quad` and `\qquad` are wide spaces that separate the formulas on one line. The Spacing lesson has the others.

### Letters and Greek

Letters are italic, the way variables are printed in books. Digits and punctuation are upright. A Greek letter is a command with its name: `\alpha`, `\beta`, `\pi`. The capitals are upright: `\Gamma`, `\Delta`, `\Omega`. A few letters have a second form, with `var` in front: `\varepsilon`, `\vartheta`, `\varphi`.

```latex
x + y = 2 \quad \alpha + \beta = \gamma \quad \Gamma(n) = (n-1)! \quad \epsilon \neq \varepsilon \quad \phi \neq \varphi \quad \Omega
```

$$
x + y = 2 \quad \alpha + \beta = \gamma \quad \Gamma(n) = (n-1)! \quad \epsilon \neq \varepsilon \quad \phi \neq \varphi \quad \Omega
$$

The angles of a triangle: $\alpha + \beta + \gamma = \pi$.

### Superscripts and subscripts

`^` raises what follows it and `_` lowers it. A single character needs no braces. Two characters or more go between braces. `^` and `_` can be used together, and one inside the other.

```latex
x^2 \quad x_i \quad x_i^2 \quad x^{10} \quad x_{i,j} \quad e^{x^2} \quad a_{n+1} = a_n + d
```

$$
x^2 \quad x_i \quad x_i^2 \quad x^{10} \quad x_{i,j} \quad e^{x^2} \quad a_{n+1} = a_n + d
$$

Euler's identity: $e^{i\pi} + 1 = 0$.

### Fractions, roots and binomials

`\frac{a}{b}` is a fraction: full size on a line of its own, small in a sentence. `\dfrac` is full size everywhere and `\tfrac` small everywhere. `\sqrt{x}` is a square root and `\sqrt[3]{x}` a cube root. `\binom{n}{k}` is a binomial coefficient. TeX's older forms `{a \over b}` and `{n \choose k}` work too.

```latex
\frac{a}{b} \quad \dfrac{a}{b} \quad \tfrac{a}{b} \quad \sqrt{2} \quad \sqrt[3]{x^3 + 1} \quad \binom{n}{k} \quad {a \over b} \quad {n \choose k}
```

$$
\frac{a}{b} \quad \dfrac{a}{b} \quad \tfrac{a}{b} \quad \sqrt{2} \quad \sqrt[3]{x^3 + 1} \quad \binom{n}{k} \quad {a \over b} \quad {n \choose k}
$$

In a sentence: $\frac{1}{2} + \dfrac{1}{3}$ and $\sqrt{2} \approx 1.414$.

The quadratic formula, with `\pm` for the plus or minus sign:

$$
x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}
$$

### Symbols

The operators between two values: `\pm`, `\times`, `\cdot`, `\div`, `\circ`, `\oplus`. The relations: `\leq`, `\geq`, `\neq`, `\approx`, `\equiv`, `\sim`, `\subset`, `\in`, `\notin`. `\not` before a relation negates it. The arrows: `\to`, `\Rightarrow`, `\leftrightarrow`, `\mapsto`. Logic and sets: `\forall`, `\exists`, `\neg`, `\land`, `\lor`, `\emptyset`, `\cup`, `\cap`, `\setminus`. The dots: `\ldots` on the line, `\cdots` at mid height, `\vdots` and `\ddots` in matrices. Then `\infty`, `\partial`, `\nabla`, `\hbar`, `\ell`. Write `\{` and `\}` to show braces. Bare braces only group and are not shown. The Reference has the full list, with the name of each symbol.

```latex
a \pm b \quad a \times b \quad a \cdot b \quad a \leq b \quad a \neq b \quad a \not\equiv b \quad a \approx b \quad x \in A \quad A \subset B
```

$$
a \pm b \quad a \times b \quad a \cdot b \quad a \leq b \quad a \neq b \quad a \not\equiv b \quad a \approx b \quad x \in A \quad A \subset B
$$

```latex
f \colon A \to B \quad x \mapsto x^2 \quad P \Rightarrow Q \quad 1, 2, \ldots, n \quad a_1 + \cdots + a_n \quad \infty \quad \partial \quad \nabla \quad \hbar \quad \ell
```

$$
f \colon A \to B \quad x \mapsto x^2 \quad P \Rightarrow Q \quad 1, 2, \ldots, n \quad a_1 + \cdots + a_n \quad \infty \quad \partial \quad \nabla \quad \hbar \quad \ell
$$

Two definitions from set theory:

$$
A \cap B = \{ x : x \in A \land x \in B \}, \qquad A \setminus B = \{ x \in A : x \notin B \}
$$

### Sums, integrals and limits

The big operators, `\sum`, `\prod`, `\int`, `\oint`, `\bigcup`, `\bigcap`, take their bounds with `_` and `^`. On a line of its own, a sum has its bounds below and above. In a sentence, they are beside it. An integral has them beside it in both cases. `\lim`, `\max`, `\min`, `\sup` and `\inf` take their condition below in the same way. `\displaystyle` at the start of a formula in a sentence makes it full size, with the bounds below and above.

```latex
\sum_{i=1}^{n} i = \frac{n(n+1)}{2} \qquad \prod_{k=1}^{n} k = n! \qquad \int_a^b f(x) \, dx \qquad \lim_{x \to 0} \frac{\sin x}{x} = 1
```

$$
\sum_{i=1}^{n} i = \frac{n(n+1)}{2} \qquad \prod_{k=1}^{n} k = n! \qquad \int_a^b f(x) \, dx \qquad \lim_{x \to 0} \frac{\sin x}{x} = 1
$$

In a sentence: $\sum_{i=1}^{n} i$ and $\lim_{x \to 0} f(x)$. With `\displaystyle`: $\displaystyle \sum_{i=1}^{n} i$.

The Basel problem, and the Gaussian integral. Printed books put a thin space, `\,`, before `dx`.

$$
\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}, \qquad \int_{-\infty}^{\infty} e^{-x^2} \, dx = \sqrt{\pi}
$$

### Functions and text

`\sin`, `\cos`, `\tan`, `\log`, `\ln`, `\exp`, `\det`, `\gcd`, `\arg` and the other function names are upright, with the right space after them. `\operatorname{Var}` makes any other name a function. `\text{...}` puts words in a formula, with their spaces. `\mathrm{d}` is an upright letter, the d of `dx` in many books. `a \equiv b \pmod{n}` adds (mod n) at the end, and `a \bmod n` writes mod between two values.

```latex
\sin^2 \theta + \cos^2 \theta = 1 \qquad \ln(xy) = \ln x + \ln y \qquad \operatorname{Var}(X) = E[X^2] - E[X]^2 \qquad a \equiv b \pmod{n}
```

$$
\sin^2 \theta + \cos^2 \theta = 1 \qquad \ln(xy) = \ln x + \ln y \qquad \operatorname{Var}(X) = E[X^2] - E[X]^2 \qquad a \equiv b \pmod{n}
$$

Conditional probability, and the derivative of the exponential:

$$
P(A \mid B) = \frac{P(A \cap B)}{P(B)} \quad \text{if } P(B) > 0, \qquad \frac{\mathrm{d}}{\mathrm{d}x} e^{x} = e^{x}
$$

### Spacing

TeX sets the spaces of a formula itself. The spaces you type are ignored. To add room: `\,` is a thin space, `\:` a medium one, `\;` a thick one, `\quad` a space as wide as the font size (1 em) and `\qquad` twice that. `\!` is a negative thin space. `~` and `\ ` add the space between two words. Inside `\text{...}`, the spaces you type are kept.

```latex
\int f(x) dx \quad \int f(x) \, dx \qquad ab \quad a\,b \quad a\:b \quad a\;b \quad a\ b \quad a \quad b \qquad \sqrt{2}\,x \quad \sqrt{2}\!x
```

$$
\int f(x) dx \quad \int f(x) \, dx \qquad ab \quad a\,b \quad a\:b \quad a\;b \quad a\ b \quad a \quad b \qquad \sqrt{2}\,x \quad \sqrt{2}\!x
$$

A definition in two parts:

$$
f(x) = x^2 \quad \text{for } x \geq 0, \qquad f(x) = -x^2 \quad \text{otherwise}
$$

### Delimiters that grow

`\left(` and `\right)` grow to the height of what they enclose. The same works with `[` and `]`, `\{` and `\}`, `|`, `\langle` and `\rangle`, `\lfloor` and `\rfloor`, `\lceil` and `\rceil`. `\left.` draws nothing. Use it when only one side has a delimiter, as in the bar of the example. To choose the size yourself: `\big(`, `\Big(`, `\bigg(`, `\Bigg(`.

```latex
\left( \frac{a}{b} \right) \quad \left[ \sum_i x_i \right] \quad \left\{ \frac{x}{2} \right\} \quad \left| \frac{x}{y} \right| \quad \left\langle u, v \right\rangle \quad \left. \frac{df}{dx} \right|_{x=0} \quad \big( \Big( \bigg( \Bigg(
```

$$
\left( \frac{a}{b} \right) \quad \left[ \sum_i x_i \right] \quad \left\{ \frac{x}{2} \right\} \quad \left| \frac{x}{y} \right| \quad \left\langle u, v \right\rangle \quad \left. \frac{df}{dx} \right|_{x=0} \quad \big( \Big( \bigg( \Bigg(
$$

The inequality of Cauchy and Schwarz:

$$
\left( \sum_{k=1}^n a_k b_k \right)^2 \leq \left( \sum_{k=1}^n a_k^2 \right) \left( \sum_{k=1}^n b_k^2 \right)
$$

### Accents and decorations

The accents: `\hat{x}`, `\bar{x}`, `\vec{v}`, `\dot{x}`, `\ddot{x}`, `\tilde{x}`, and the wide ones that stretch over a group, `\widehat{abc}`, `\widetilde{abc}`, `\overrightarrow{AB}`. `\overline{z}` and `\underline{x}` draw a line. `\overbrace{...}^{n}` and `\underbrace{...}_{n}` draw a brace with a label. `\overset{a}{b}` puts a over b, and `\underset{a}{b}` puts a under b. `\stackrel{\text{def}}{=}` puts its first argument over a relation. `\xrightarrow{f}` is an arrow that grows with its label. `\phantom{x}` leaves an empty space the size of x, to align two lines.

```latex
\hat{x} \quad \bar{x} \quad \vec{v} \quad \dot{x} \quad \ddot{x} \quad \tilde{x} \quad \widehat{abc} \quad \widetilde{abc} \quad \overrightarrow{AB} \quad \overline{z} \quad \underline{x}
```

$$
\hat{x} \quad \bar{x} \quad \vec{v} \quad \dot{x} \quad \ddot{x} \quad \tilde{x} \quad \widehat{abc} \quad \widetilde{abc} \quad \overrightarrow{AB} \quad \overline{z} \quad \underline{x}
$$

```latex
\underbrace{a + a + \cdots + a}_{n} \quad \overbrace{1 + 2 + 3}^{6} \quad a \overset{!}{=} b \quad x \stackrel{\text{def}}{=} y \quad A \xrightarrow{f} B \quad x\phantom{xx}y
```

$$
\underbrace{a + a + \cdots + a}_{n} \quad \overbrace{1 + 2 + 3}^{6} \quad a \overset{!}{=} b \quad x \stackrel{\text{def}}{=} y \quad A \xrightarrow{f} B \quad x\phantom{xx}y
$$

The Schrödinger equation:

$$
i\hbar \frac{\partial}{\partial t} \Psi(x, t) = \hat{H} \Psi(x, t)
$$

### Alphabets and styles

`\mathbb{R}` is the double-struck R of the real numbers. `\mathbb{N}`, `\mathbb{Z}`, `\mathbb{Q}` and `\mathbb{C}` work the same way. `\mathcal{L}` is calligraphic, `\mathfrak{g}` Fraktur, `\mathbf{v}` bold, `\boldsymbol{\alpha}` bold for a Greek letter, `\mathsf{T}` sans serif, `\mathtt{x}` typewriter, `\mathrm{d}` upright. TeX's switches `{\bf x}` and `{\rm d}` act until the end of their group. The style switches `\displaystyle`, `\textstyle`, `\scriptstyle` and `\scriptscriptstyle` change the size until the end of the group.

```latex
\mathbb{R} \quad \mathbb{N} \quad \mathcal{L} \quad \mathfrak{g} \quad \mathbf{v} \quad \boldsymbol{\alpha} \quad \mathsf{T} \quad \mathtt{x} \quad \mathrm{d} \quad {\bf x} \quad {\rm d} \quad {\scriptstyle \sum_i x_i}
```

$$
\mathbb{R} \quad \mathbb{N} \quad \mathcal{L} \quad \mathfrak{g} \quad \mathbf{v} \quad \boldsymbol{\alpha} \quad \mathsf{T} \quad \mathtt{x} \quad \mathrm{d} \quad {\bf x} \quad {\rm d} \quad {\scriptstyle \sum_i x_i}
$$

The norm of a vector:

$$
f \colon \mathbb{R}^n \to \mathbb{R}, \qquad \mathbf{x} \mapsto \lVert \mathbf{x} \rVert = \sqrt{x_1^2 + \cdots + x_n^2}
$$

### Matrices, cases and aligned equations

A matrix is an environment, `\begin{pmatrix}` to `\end{pmatrix}`, with `&` between the columns and `\\` at the end of a row. `pmatrix` has parentheses, `bmatrix` brackets, `Bmatrix` braces, `vmatrix` bars, `Vmatrix` double bars, `matrix` nothing, and `smallmatrix` fits in a sentence. `cases` is a definition by cases, with one brace on the left. `aligned` lines up several equations on their `&`, placed before the equals sign. `array` takes a letter per column, `l`, `c` or `r`, to align it left, center or right.

```latex
\begin{pmatrix} a & b \\ c & d \end{pmatrix} \quad \begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix} \quad \begin{vmatrix} a & b \\ c & d \end{vmatrix} \quad \begin{Bmatrix} x \\ y \end{Bmatrix} \quad \begin{array}{lcr} a & b & c \\ 1 & 2 & 3 \end{array}
```

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix} \quad \begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix} \quad \begin{vmatrix} a & b \\ c & d \end{vmatrix} \quad \begin{Bmatrix} x \\ y \end{Bmatrix} \quad \begin{array}{lcr} a & b & c \\ 1 & 2 & 3 \end{array}
$$

In a sentence, a small matrix: $\begin{smallmatrix} 1 & 0 \\ 0 & 1 \end{smallmatrix}$.

```latex
|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases} \qquad \begin{aligned} (a+b)^2 &= (a+b)(a+b) \\ &= a^2 + 2ab + b^2 \end{aligned}
```

$$
|x| = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases} \qquad \begin{aligned} (a+b)^2 &= (a+b)(a+b) \\ &= a^2 + 2ab + b^2 \end{aligned}
$$

The rotation matrix:

$$
R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}
$$

### Color

`\color{red}` colors the rest of its group, and `\textcolor{blue}{x}` colors its argument alone. The 19 color names of the Reference work, and so do the `#rgb` and `#rrggbb` forms. Colors do not change with the theme.

```latex
{\color{red} a^2} + {\color{blue} b^2} = \textcolor{#2a9d8f}{c^2}
```

$$
{\color{red} a^2} + {\color{blue} b^2} = \textcolor{#2a9d8f}{c^2}
$$

### Macros

`\newcommand{\name}{body}` defines a command. The command works until the end of the formula. `[1]` after the name gives it a parameter, written `#1` in the body. A command can have up to nine. A second bracket gives the first parameter a default value, and a call can then leave it out. `\renewcommand` gives a name a new definition.

```latex
\newcommand{\avg}[1]{\left\langle #1 \right\rangle} \operatorname{Var}(X) = \avg{X^2} - \avg{X}^2 \qquad \newcommand{\norm}[2][2]{\lVert #2 \rVert_{#1}} \norm{v} \quad \norm[1]{v} \quad \norm[\infty]{v}
```

$$
\newcommand{\avg}[1]{\left\langle #1 \right\rangle} \operatorname{Var}(X) = \avg{X^2} - \avg{X}^2 \qquad \newcommand{\norm}[2][2]{\lVert #2 \rVert_{#1}} \norm{v} \quad \norm[1]{v} \quad \norm[\infty]{v}
$$

### A command Oryx does not know

A command that Oryx does not know is shown as written, in the code font. The rest of the formula is typeset. A macro that calls itself without end is stopped after a limit, and is shown as written too. The Reference below lists every command Oryx knows. Most of them come from KaTeX, with the names of Wikipedia's math added.

```latex
x = \undefinedop{y} + z^2 \qquad \newcommand{\again}{\again} \again
```

$$
x = \undefinedop{y} + z^2 \qquad \newcommand{\again}{\again} \again
$$

## Reference

Every command Oryx knows, by family. In Oryx, the Shows column is typeset by Oryx itself, so a command renders here as it renders in your document. The From column names the first of these sources that defines the command: TeX is Knuth's plain TeX, LaTeX its kernel with the color and bm packages, AMS the amsmath and amssymb packages, MediaWiki the math of Wikipedia, KaTeX the web library most of Oryx's commands come from. Oryx marks a command that none of them defines.

### Greek letters

Lower case first, each variant right after its letter, then the capitals.

| Command | Shows | Name | From |
|---|---|---|---|
| `\alpha` | $\alpha$ | greek small letter alpha | TeX |
| `\beta` | $\beta$ | greek small letter beta | TeX |
| `\gamma` | $\gamma$ | greek small letter gamma | TeX |
| `\delta` | $\delta$ | greek small letter delta | TeX |
| `\epsilon` | $\epsilon$ | greek lunate epsilon symbol | TeX |
| `\varepsilon` | $\varepsilon$ | greek small letter epsilon | TeX |
| `\zeta` | $\zeta$ | greek small letter zeta | TeX |
| `\eta` | $\eta$ | greek small letter eta | TeX |
| `\theta` | $\theta$ | greek small letter theta | TeX |
| `\vartheta` | $\vartheta$ | greek theta symbol | TeX |
| `\thetasym` | $\thetasym$ | greek theta symbol | MediaWiki |
| `\iota` | $\iota$ | greek small letter iota | TeX |
| `\kappa` | $\kappa$ | greek small letter kappa | TeX |
| `\varkappa` | $\varkappa$ | greek kappa symbol | AMS |
| `\lambda` | $\lambda$ | greek small letter lamda | TeX |
| `\mu` | $\mu$ | greek small letter mu | TeX |
| `\nu` | $\nu$ | greek small letter nu | TeX |
| `\xi` | $\xi$ | greek small letter xi | TeX |
| `\omicron` | $\omicron$ | greek small letter omicron | MediaWiki |
| `\pi` | $\pi$ | greek small letter pi | TeX |
| `\varpi` | $\varpi$ | greek pi symbol | TeX |
| `\rho` | $\rho$ | greek small letter rho | TeX |
| `\varrho` | $\varrho$ | greek rho symbol | TeX |
| `\sigma` | $\sigma$ | greek small letter sigma | TeX |
| `\varsigma` | $\varsigma$ | greek small letter final sigma | TeX |
| `\tau` | $\tau$ | greek small letter tau | TeX |
| `\upsilon` | $\upsilon$ | greek small letter upsilon | TeX |
| `\phi` | $\phi$ | greek phi symbol | TeX |
| `\varphi` | $\varphi$ | greek small letter phi | TeX |
| `\chi` | $\chi$ | greek small letter chi | TeX |
| `\psi` | $\psi$ | greek small letter psi | TeX |
| `\omega` | $\omega$ | greek small letter omega | TeX |
| `\digamma` | $\digamma$ | greek small letter digamma | AMS |
| `\Alpha` | $\Alpha$ | greek capital letter alpha | MediaWiki |
| `\Beta` | $\Beta$ | greek capital letter beta | MediaWiki |
| `\Gamma` | $\Gamma$ | greek capital letter gamma | TeX |
| `\varGamma` | $\varGamma$ | italic capital gamma | AMS |
| `\Delta` | $\Delta$ | greek capital letter delta | TeX |
| `\varDelta` | $\varDelta$ | italic capital delta | AMS |
| `\Epsilon` | $\Epsilon$ | greek capital letter epsilon | MediaWiki |
| `\Zeta` | $\Zeta$ | greek capital letter zeta | MediaWiki |
| `\Eta` | $\Eta$ | greek capital letter eta | MediaWiki |
| `\Theta` | $\Theta$ | greek capital letter theta | TeX |
| `\varTheta` | $\varTheta$ | italic capital theta | AMS |
| `\Iota` | $\Iota$ | greek capital letter iota | MediaWiki |
| `\Kappa` | $\Kappa$ | greek capital letter kappa | MediaWiki |
| `\Lambda` | $\Lambda$ | greek capital letter lamda | TeX |
| `\varLambda` | $\varLambda$ | italic capital lamda | AMS |
| `\Mu` | $\Mu$ | greek capital letter mu | MediaWiki |
| `\Nu` | $\Nu$ | greek capital letter nu | MediaWiki |
| `\Xi` | $\Xi$ | greek capital letter xi | TeX |
| `\varXi` | $\varXi$ | italic capital xi | AMS |
| `\Omicron` | $\Omicron$ | greek capital letter omicron | MediaWiki |
| `\Pi` | $\Pi$ | greek capital letter pi | TeX |
| `\varPi` | $\varPi$ | italic capital pi | AMS |
| `\Rho` | $\Rho$ | greek capital letter rho | MediaWiki |
| `\Sigma` | $\Sigma$ | greek capital letter sigma | TeX |
| `\varSigma` | $\varSigma$ | italic capital sigma | AMS |
| `\Tau` | $\Tau$ | greek capital letter tau | MediaWiki |
| `\Upsilon` | $\Upsilon$ | greek capital letter upsilon | TeX |
| `\varUpsilon` | $\varUpsilon$ | italic capital upsilon | AMS |
| `\Phi` | $\Phi$ | greek capital letter phi | TeX |
| `\varPhi` | $\varPhi$ | italic capital phi | AMS |
| `\Chi` | $\Chi$ | greek capital letter chi | MediaWiki |
| `\Psi` | $\Psi$ | greek capital letter psi | TeX |
| `\Omega` | $\Omega$ | greek capital letter omega | TeX |
| `\varOmega` | $\varOmega$ | italic capital omega | AMS |

### Letters and letterlike symbols

| Command | Shows | Name | From |
|---|---|---|---|
| `\alef` | $\alef$ | alef symbol | MediaWiki |
| `\alefsym` | $\alefsym$ | alef symbol | MediaWiki |
| `\aleph` | $\aleph$ | alef symbol | TeX |
| `\Bbbk` | $\Bbbk$ | double-struck small k | AMS |
| `\beth` | $\beth$ | bet symbol | AMS |
| `\C` | $\C$ | double-struck capital c | MediaWiki |
| `\cnums` | $\cnums$ | double-struck capital c | MediaWiki |
| `\Complex` | $\Complex$ | double-struck capital c | MediaWiki |
| `\daleth` | $\daleth$ | dalet symbol | AMS |
| `\ell` | $\ell$ | script small l | TeX |
| `\Finv` | $\Finv$ | turned capital f | AMS |
| `\Game` | $\Game$ | turned sans-serif capital g | AMS |
| `\gimel` | $\gimel$ | gimel symbol | AMS |
| `\H` | $\H$ | double-struck capital h | MediaWiki |
| `\hbar` | $\hbar$ | planck constant over two pi | TeX |
| `\hslash` | $\hslash$ | planck constant over two pi | AMS |
| `\Im` | $\Im$ | black-letter capital i | TeX |
| `\image` | $\image$ | black-letter capital i | MediaWiki |
| `\imath` | $\imath$ | italic small dotless i | TeX |
| `\jmath` | $\jmath$ | italic small dotless j | TeX |
| `\mho` | $\mho$ | inverted ohm sign | AMS |
| `\N` | $\N$ | double-struck capital n | MediaWiki |
| `\natnums` | $\natnums$ | double-struck capital n | MediaWiki |
| `\Q` | $\Q$ | double-struck capital q | MediaWiki |
| `\R` | $\R$ | double-struck capital r | MediaWiki |
| `\Re` | $\Re$ | black-letter capital r | TeX |
| `\real` | $\real$ | black-letter capital r | MediaWiki |
| `\Reals` | $\Reals$ | double-struck capital r | MediaWiki |
| `\reals` | $\reals$ | double-struck capital r | MediaWiki |
| `\weierp` | $\weierp$ | script capital p | MediaWiki |
| `\wp` | $\wp$ | script capital p | TeX |
| `\Z` | $\Z$ | double-struck capital z | MediaWiki |

### Binary operators

| Command | Shows | Name | From |
|---|---|---|---|
| `\amalg` | $\amalg$ | amalgamation or coproduct | TeX |
| `\And` | $\And$ | ampersand | AMS |
| `\and` | $\and$ | logical and | MediaWiki |
| `\ast` | $\ast$ | asterisk operator | TeX |
| `\barwedge` | $\barwedge$ | nand | AMS |
| `\bigcirc` | $\bigcirc$ | large circle | TeX |
| `\bigtriangledown` | $\bigtriangledown$ | white down-pointing triangle | TeX |
| `\bigtriangleup` | $\bigtriangleup$ | white up-pointing triangle | TeX |
| `\blacktriangleleft` | $\blacktriangleleft$ | black left-pointing triangle | AMS |
| `\blacktriangleright` | $\blacktriangleright$ | black right-pointing triangle | AMS |
| `\boxdot` | $\boxdot$ | squared dot operator | AMS |
| `\boxminus` | $\boxminus$ | squared minus | AMS |
| `\boxplus` | $\boxplus$ | squared plus | AMS |
| `\boxtimes` | $\boxtimes$ | squared times | AMS |
| `\bull` | $\bull$ | bullet operator | MediaWiki |
| `\bullet` | $\bullet$ | bullet operator | TeX |
| `\Cap` | $\Cap$ | double intersection | AMS |
| `\cap` | $\cap$ | intersection | TeX |
| `\cdot` | $\cdot$ | dot operator | TeX |
| `\centerdot` | $\centerdot$ | dot operator | AMS |
| `\circ` | $\circ$ | ring operator | TeX |
| `\circledast` | $\circledast$ | circled asterisk operator | AMS |
| `\circledcirc` | $\circledcirc$ | circled ring operator | AMS |
| `\circleddash` | $\circleddash$ | circled dash | AMS |
| `\Cup` | $\Cup$ | double union | AMS |
| `\cup` | $\cup$ | union | TeX |
| `\curlyvee` | $\curlyvee$ | curly logical or | AMS |
| `\curlywedge` | $\curlywedge$ | curly logical and | AMS |
| `\Dagger` | $\Dagger$ | double dagger | MediaWiki |
| `\dagger` | $\dagger$ | dagger | TeX |
| `\ddagger` | $\ddagger$ | double dagger | TeX |
| `\diamond` | $\diamond$ | diamond operator | TeX |
| `\div` | $\div$ | division sign | TeX |
| `\divideontimes` | $\divideontimes$ | division times | AMS |
| `\dotplus` | $\dotplus$ | dot plus | AMS |
| `\doublebarwedge` | $\doublebarwedge$ | logical and with double overbar | AMS |
| `\doublecap` | $\doublecap$ | double intersection | AMS |
| `\doublecup` | $\doublecup$ | double union | AMS |
| `\intercal` | $\intercal$ | intercalate | AMS |
| `\land` | $\land$ | logical and | TeX |
| `\leftthreetimes` | $\leftthreetimes$ | left semidirect product | AMS |
| `\lhd` | $\lhd$ | normal subgroup of | AMS |
| `\lor` | $\lor$ | logical or | TeX |
| `\ltimes` | $\ltimes$ | left normal factor semidirect product | AMS |
| `\mp` | $\mp$ | minus-or-plus sign | TeX |
| `\odot` | $\odot$ | circled dot operator | TeX |
| `\ominus` | $\ominus$ | circled minus | TeX |
| `\oplus` | $\oplus$ | circled plus | TeX |
| `\or` | $\or$ | logical or | MediaWiki |
| `\oslash` | $\oslash$ | circled division slash | TeX |
| `\otimes` | $\otimes$ | circled times | TeX |
| `\plusmn` | $\plusmn$ | plus-minus sign | MediaWiki |
| `\pm` | $\pm$ | plus-minus sign | TeX |
| `\rhd` | $\rhd$ | contains as normal subgroup | AMS |
| `\rightthreetimes` | $\rightthreetimes$ | right semidirect product | AMS |
| `\rtimes` | $\rtimes$ | right normal factor semidirect product | AMS |
| `\sdot` | $\sdot$ | dot operator | MediaWiki |
| `\setminus` | $\setminus$ | set minus | TeX |
| `\smallsetminus` | $\smallsetminus$ | set minus | AMS |
| `\sqcap` | $\sqcap$ | square cap | TeX |
| `\sqcup` | $\sqcup$ | square cup | TeX |
| `\star` | $\star$ | star operator | TeX |
| `\times` | $\times$ | multiplication sign | TeX |
| `\triangleleft` | $\triangleleft$ | white left-pointing small triangle | TeX |
| `\triangleright` | $\triangleright$ | white right-pointing small triangle | TeX |
| `\unlhd` | $\unlhd$ | normal subgroup of or equal to | AMS |
| `\unrhd` | $\unrhd$ | contains as normal subgroup or equal to | AMS |
| `\uplus` | $\uplus$ | multiset union | TeX |
| `\vee` | $\vee$ | logical or | TeX |
| `\veebar` | $\veebar$ | xor | AMS |
| `\wedge` | $\wedge$ | logical and | TeX |
| `\wr` | $\wr$ | wreath product | TeX |

### Relations

| Command | Shows | Name | From |
|---|---|---|---|
| `\approx` | $\approx$ | almost equal to | TeX |
| `\approxeq` | $\approxeq$ | almost equal or equal to | AMS |
| `\asymp` | $\asymp$ | equivalent to | TeX |
| `\backepsilon` | $\backepsilon$ | small contains as member | AMS |
| `\backsim` | $\backsim$ | reversed tilde | AMS |
| `\backsimeq` | $\backsimeq$ | reversed tilde equals | AMS |
| `\because` | $\because$ | because | AMS |
| `\between` | $\between$ | between | AMS |
| `\bowtie` | $\bowtie$ | bowtie | TeX |
| `\Bumpeq` | $\Bumpeq$ | geometrically equivalent to | AMS |
| `\bumpeq` | $\bumpeq$ | difference between | AMS |
| `\circeq` | $\circeq$ | ring equal to | AMS |
| `\coloneqq` | $\coloneqq$ | colon equals | KaTeX |
| `\cong` | $\cong$ | approximately equal to | TeX |
| `\curlyeqprec` | $\curlyeqprec$ | equal to or precedes | AMS |
| `\curlyeqsucc` | $\curlyeqsucc$ | equal to or succeeds | AMS |
| `\dashv` | $\dashv$ | left tack | TeX |
| `\Doteq` | $\Doteq$ | geometrically equal to | AMS |
| `\doteq` | $\doteq$ | approaches the limit | TeX |
| `\doteqdot` | $\doteqdot$ | geometrically equal to | AMS |
| `\eqcirc` | $\eqcirc$ | ring in equal to | AMS |
| `\eqqcolon` | $\eqqcolon$ | equals colon | KaTeX |
| `\eqsim` | $\eqsim$ | minus tilde | AMS |
| `\eqslantgtr` | $\eqslantgtr$ | slanted equal to or greater-than | AMS |
| `\eqslantless` | $\eqslantless$ | slanted equal to or less-than | AMS |
| `\equiv` | $\equiv$ | identical to | TeX |
| `\fallingdotseq` | $\fallingdotseq$ | approximately equal to or the image of | AMS |
| `\frown` | $\frown$ | frown | TeX |
| `\ge` | $\ge$ | greater-than or equal to | TeX |
| `\geq` | $\geq$ | greater-than or equal to | TeX |
| `\geqq` | $\geqq$ | greater-than over equal to | AMS |
| `\geqslant` | $\geqslant$ | greater-than or slanted equal to | AMS |
| `\gg` | $\gg$ | much greater-than | TeX |
| `\ggg` | $\ggg$ | very much greater-than | AMS |
| `\gggtr` | $\gggtr$ | very much greater-than | AMS |
| `\gt` | $\gt$ | greater-than sign | MediaWiki |
| `\gtrapprox` | $\gtrapprox$ | greater-than or approximate | AMS |
| `\gtrdot` | $\gtrdot$ | greater-than with dot | AMS |
| `\gtreqless` | $\gtreqless$ | greater-than equal to or less-than | AMS |
| `\gtreqqless` | $\gtreqqless$ | greater-than above double-line equal above less-than | AMS |
| `\gtrless` | $\gtrless$ | greater-than or less-than | AMS |
| `\gtrsim` | $\gtrsim$ | greater-than or equivalent to | AMS |
| `\in` | $\in$ | element of | TeX |
| `\isin` | $\isin$ | element of | MediaWiki |
| `\Join` | $\Join$ | bowtie | AMS |
| `\le` | $\le$ | less-than or equal to | TeX |
| `\leq` | $\leq$ | less-than or equal to | TeX |
| `\leqq` | $\leqq$ | less-than over equal to | AMS |
| `\leqslant` | $\leqslant$ | less-than or slanted equal to | AMS |
| `\lessapprox` | $\lessapprox$ | less-than or approximate | AMS |
| `\lessdot` | $\lessdot$ | less-than with dot | AMS |
| `\lesseqgtr` | $\lesseqgtr$ | less-than equal to or greater-than | AMS |
| `\lesseqqgtr` | $\lesseqqgtr$ | less-than above double-line equal above greater-than | AMS |
| `\lessgtr` | $\lessgtr$ | less-than or greater-than | AMS |
| `\lesssim` | $\lesssim$ | less-than or equivalent to | AMS |
| `\ll` | $\ll$ | much less-than | TeX |
| `\lll` | $\lll$ | very much less-than | AMS |
| `\llless` | $\llless$ | very much less-than | AMS |
| `\lt` | $\lt$ | less-than sign | MediaWiki |
| `\mid` | $\mid$ | divides | TeX |
| `\models` | $\models$ | true | TeX |
| `\multimap` | $\multimap$ | multimap | AMS |
| `\ni` | $\ni$ | contains as member | TeX |
| `\owns` | $\owns$ | contains as member | TeX |
| `\parallel` | $\parallel$ | parallel to | TeX |
| `\perp` | $\perp$ | up tack | TeX |
| `\pitchfork` | $\pitchfork$ | pitchfork | AMS |
| `\prec` | $\prec$ | precedes | TeX |
| `\precapprox` | $\precapprox$ | precedes above almost equal to | AMS |
| `\preccurlyeq` | $\preccurlyeq$ | precedes or equal to | AMS |
| `\preceq` | $\preceq$ | precedes above single-line equals sign | TeX |
| `\precsim` | $\precsim$ | precedes or equivalent to | AMS |
| `\propto` | $\propto$ | proportional to | TeX |
| `\risingdotseq` | $\risingdotseq$ | image of or approximately equal to | AMS |
| `\shortmid` | $\shortmid$ | divides | AMS |
| `\shortparallel` | $\shortparallel$ | parallel to | AMS |
| `\sim` | $\sim$ | tilde operator | TeX |
| `\simeq` | $\simeq$ | asymptotically equal to | TeX |
| `\smallfrown` | $\smallfrown$ | frown | AMS |
| `\smallsmile` | $\smallsmile$ | smile | AMS |
| `\smile` | $\smile$ | smile | TeX |
| `\sqsubset` | $\sqsubset$ | square image of | AMS |
| `\sqsubseteq` | $\sqsubseteq$ | square image of or equal to | TeX |
| `\sqsupset` | $\sqsupset$ | square original of | AMS |
| `\sqsupseteq` | $\sqsupseteq$ | square original of or equal to | TeX |
| `\sub` | $\sub$ | subset of | MediaWiki |
| `\sube` | $\sube$ | subset of or equal to | MediaWiki |
| `\Subset` | $\Subset$ | double subset | AMS |
| `\subset` | $\subset$ | subset of | TeX |
| `\subseteq` | $\subseteq$ | subset of or equal to | TeX |
| `\subseteqq` | $\subseteqq$ | subset of above equals sign | AMS |
| `\succ` | $\succ$ | succeeds | TeX |
| `\succapprox` | $\succapprox$ | succeeds above almost equal to | AMS |
| `\succcurlyeq` | $\succcurlyeq$ | succeeds or equal to | AMS |
| `\succeq` | $\succeq$ | succeeds above single-line equals sign | TeX |
| `\succsim` | $\succsim$ | succeeds or equivalent to | AMS |
| `\supe` | $\supe$ | superset of or equal to | MediaWiki |
| `\Supset` | $\Supset$ | double superset | AMS |
| `\supset` | $\supset$ | superset of | TeX |
| `\supseteq` | $\supseteq$ | superset of or equal to | TeX |
| `\supseteqq` | $\supseteqq$ | superset of above equals sign | AMS |
| `\therefore` | $\therefore$ | therefore | AMS |
| `\thickapprox` | $\thickapprox$ | almost equal to | AMS |
| `\thicksim` | $\thicksim$ | tilde operator | AMS |
| `\trianglelefteq` | $\trianglelefteq$ | normal subgroup of or equal to | AMS |
| `\triangleq` | $\triangleq$ | delta equal to | AMS |
| `\trianglerighteq` | $\trianglerighteq$ | contains as normal subgroup or equal to | AMS |
| `\varpropto` | $\varpropto$ | proportional to | AMS |
| `\vartriangle` | $\vartriangle$ | white up-pointing triangle | AMS |
| `\vartriangleleft` | $\vartriangleleft$ | normal subgroup of | AMS |
| `\vartriangleright` | $\vartriangleright$ | contains as normal subgroup | AMS |
| `\Vdash` | $\Vdash$ | forces | AMS |
| `\vDash` | $\vDash$ | true | AMS |
| `\vdash` | $\vdash$ | right tack | TeX |
| `\Vvdash` | $\Vvdash$ | triple vertical bar right turnstile | AMS |

### Negated relations

| Command | Shows | Name | From |
|---|---|---|---|
| `\gnapprox` | $\gnapprox$ | greater-than and not approximate | AMS |
| `\gneq` | $\gneq$ | greater-than and single-line not equal to | AMS |
| `\gneqq` | $\gneqq$ | greater-than but not equal to | AMS |
| `\gnsim` | $\gnsim$ | greater-than but not equivalent to | AMS |
| `\lnapprox` | $\lnapprox$ | less-than and not approximate | AMS |
| `\lneq` | $\lneq$ | less-than and single-line not equal to | AMS |
| `\lneqq` | $\lneqq$ | less-than but not equal to | AMS |
| `\lnsim` | $\lnsim$ | less-than but not equivalent to | AMS |
| `\ncong` | $\ncong$ | neither approximately nor actually equal to | AMS |
| `\ne` | $\ne$ | not equal to | TeX |
| `\neq` | $\neq$ | not equal to | TeX |
| `\ngeq` | $\ngeq$ | neither greater-than nor equal to | AMS |
| `\ngtr` | $\ngtr$ | not greater-than | AMS |
| `\nleq` | $\nleq$ | neither less-than nor equal to | AMS |
| `\nless` | $\nless$ | not less-than | AMS |
| `\nmid` | $\nmid$ | does not divide | AMS |
| `\notin` | $\notin$ | not an element of | TeX |
| `\notni` | $\notni$ | does not contain as member | KaTeX |
| `\nparallel` | $\nparallel$ | not parallel to | AMS |
| `\nprec` | $\nprec$ | does not precede | AMS |
| `\npreceq` | $\npreceq$ | does not precede or equal | AMS |
| `\nsim` | $\nsim$ | not tilde | AMS |
| `\nsubseteq` | $\nsubseteq$ | neither a subset of nor equal to | AMS |
| `\nsucc` | $\nsucc$ | does not succeed | AMS |
| `\nsucceq` | $\nsucceq$ | does not succeed or equal | AMS |
| `\nsupseteq` | $\nsupseteq$ | neither a superset of nor equal to | AMS |
| `\ntriangleleft` | $\ntriangleleft$ | not normal subgroup of | AMS |
| `\ntrianglelefteq` | $\ntrianglelefteq$ | not normal subgroup of or equal to | AMS |
| `\ntriangleright` | $\ntriangleright$ | does not contain as normal subgroup | AMS |
| `\ntrianglerighteq` | $\ntrianglerighteq$ | does not contain as normal subgroup or equal | AMS |
| `\nVDash` | $\nVDash$ | negated double vertical bar double right turnstile | AMS |
| `\nVdash` | $\nVdash$ | does not force | AMS |
| `\nvDash` | $\nvDash$ | not true | AMS |
| `\nvdash` | $\nvdash$ | does not prove | AMS |
| `\precnapprox` | $\precnapprox$ | precedes above not almost equal to | AMS |
| `\precneqq` | $\precneqq$ | precedes above not equal to | AMS |
| `\precnsim` | $\precnsim$ | precedes but not equivalent to | AMS |
| `\subsetneq` | $\subsetneq$ | subset of with not equal to | AMS |
| `\subsetneqq` | $\subsetneqq$ | subset of above not equal to | AMS |
| `\succnapprox` | $\succnapprox$ | succeeds above not almost equal to | AMS |
| `\succneqq` | $\succneqq$ | succeeds above not equal to | AMS |
| `\succnsim` | $\succnsim$ | succeeds but not equivalent to | AMS |
| `\supsetneq` | $\supsetneq$ | superset of with not equal to | AMS |
| `\supsetneqq` | $\supsetneqq$ | superset of above not equal to | AMS |

### Arrows

| Command | Shows | Name | From |
|---|---|---|---|
| `\circlearrowleft` | $\circlearrowleft$ | anticlockwise open circle arrow | AMS |
| `\circlearrowright` | $\circlearrowright$ | clockwise open circle arrow | AMS |
| `\curvearrowleft` | $\curvearrowleft$ | anticlockwise top semicircle arrow | AMS |
| `\curvearrowright` | $\curvearrowright$ | clockwise top semicircle arrow | AMS |
| `\Darr` | $\Darr$ | downwards double arrow | MediaWiki |
| `\dArr` | $\dArr$ | downwards double arrow | MediaWiki |
| `\darr` | $\darr$ | downwards arrow | MediaWiki |
| `\dashleftarrow` | $\dashleftarrow$ | leftwards dashed arrow | AMS |
| `\dashrightarrow` | $\dashrightarrow$ | rightwards dashed arrow | AMS |
| `\Downarrow` | $\Downarrow$ | downwards double arrow | TeX |
| `\downarrow` | $\downarrow$ | downwards arrow | TeX |
| `\downdownarrows` | $\downdownarrows$ | downwards paired arrows | AMS |
| `\downharpoonleft` | $\downharpoonleft$ | downwards harpoon with barb leftwards | AMS |
| `\downharpoonright` | $\downharpoonright$ | downwards harpoon with barb rightwards | AMS |
| `\gets` | $\gets$ | leftwards arrow | TeX |
| `\Harr` | $\Harr$ | left right double arrow | MediaWiki |
| `\hArr` | $\hArr$ | left right double arrow | MediaWiki |
| `\harr` | $\harr$ | left right arrow | MediaWiki |
| `\hookleftarrow` | $\hookleftarrow$ | leftwards arrow with hook | TeX |
| `\hookrightarrow` | $\hookrightarrow$ | rightwards arrow with hook | TeX |
| `\iff` | $\iff$ | long left right double arrow | TeX |
| `\impliedby` | $\impliedby$ | long leftwards double arrow | AMS |
| `\implies` | $\implies$ | long rightwards double arrow | AMS |
| `\Larr` | $\Larr$ | leftwards double arrow | MediaWiki |
| `\lArr` | $\lArr$ | leftwards double arrow | MediaWiki |
| `\larr` | $\larr$ | leftwards arrow | MediaWiki |
| `\leadsto` | $\leadsto$ | rightwards squiggle arrow | AMS |
| `\Leftarrow` | $\Leftarrow$ | leftwards double arrow | TeX |
| `\leftarrow` | $\leftarrow$ | leftwards arrow | TeX |
| `\leftarrowtail` | $\leftarrowtail$ | leftwards arrow with tail | AMS |
| `\leftharpoondown` | $\leftharpoondown$ | leftwards harpoon with barb downwards | TeX |
| `\leftharpoonup` | $\leftharpoonup$ | leftwards harpoon with barb upwards | TeX |
| `\leftleftarrows` | $\leftleftarrows$ | leftwards paired arrows | AMS |
| `\Leftrightarrow` | $\Leftrightarrow$ | left right double arrow | TeX |
| `\leftrightarrow` | $\leftrightarrow$ | left right arrow | TeX |
| `\leftrightarrows` | $\leftrightarrows$ | leftwards arrow over rightwards arrow | AMS |
| `\leftrightharpoons` | $\leftrightharpoons$ | leftwards harpoon over rightwards harpoon | AMS |
| `\leftrightsquigarrow` | $\leftrightsquigarrow$ | left right wave arrow | AMS |
| `\Lleftarrow` | $\Lleftarrow$ | leftwards triple arrow | AMS |
| `\Longleftarrow` | $\Longleftarrow$ | long leftwards double arrow | TeX |
| `\longleftarrow` | $\longleftarrow$ | long leftwards arrow | TeX |
| `\Longleftrightarrow` | $\Longleftrightarrow$ | long left right double arrow | TeX |
| `\longleftrightarrow` | $\longleftrightarrow$ | long left right arrow | TeX |
| `\longmapsto` | $\longmapsto$ | long rightwards arrow from bar | TeX |
| `\Longrightarrow` | $\Longrightarrow$ | long rightwards double arrow | TeX |
| `\longrightarrow` | $\longrightarrow$ | long rightwards arrow | TeX |
| `\looparrowleft` | $\looparrowleft$ | leftwards arrow with loop | AMS |
| `\looparrowright` | $\looparrowright$ | rightwards arrow with loop | AMS |
| `\Lrarr` | $\Lrarr$ | left right double arrow | MediaWiki |
| `\lrArr` | $\lrArr$ | left right double arrow | MediaWiki |
| `\lrarr` | $\lrarr$ | left right arrow | MediaWiki |
| `\Lsh` | $\Lsh$ | upwards arrow with tip leftwards | AMS |
| `\mapsto` | $\mapsto$ | rightwards arrow from bar | TeX |
| `\nearrow` | $\nearrow$ | north east arrow | TeX |
| `\nLeftarrow` | $\nLeftarrow$ | leftwards double arrow with stroke | AMS |
| `\nleftarrow` | $\nleftarrow$ | leftwards arrow with stroke | AMS |
| `\nLeftrightarrow` | $\nLeftrightarrow$ | left right double arrow with stroke | AMS |
| `\nleftrightarrow` | $\nleftrightarrow$ | left right arrow with stroke | AMS |
| `\nRightarrow` | $\nRightarrow$ | rightwards double arrow with stroke | AMS |
| `\nrightarrow` | $\nrightarrow$ | rightwards arrow with stroke | AMS |
| `\nwarrow` | $\nwarrow$ | north west arrow | TeX |
| `\Rarr` | $\Rarr$ | rightwards double arrow | MediaWiki |
| `\rArr` | $\rArr$ | rightwards double arrow | MediaWiki |
| `\rarr` | $\rarr$ | rightwards arrow | MediaWiki |
| `\restriction` | $\restriction$ | upwards harpoon with barb rightwards | AMS |
| `\Rightarrow` | $\Rightarrow$ | rightwards double arrow | TeX |
| `\rightarrow` | $\rightarrow$ | rightwards arrow | TeX |
| `\rightarrowtail` | $\rightarrowtail$ | rightwards arrow with tail | AMS |
| `\rightharpoondown` | $\rightharpoondown$ | rightwards harpoon with barb downwards | TeX |
| `\rightharpoonup` | $\rightharpoonup$ | rightwards harpoon with barb upwards | TeX |
| `\rightleftarrows` | $\rightleftarrows$ | rightwards arrow over leftwards arrow | AMS |
| `\rightleftharpoons` | $\rightleftharpoons$ | rightwards harpoon over leftwards harpoon | TeX |
| `\rightrightarrows` | $\rightrightarrows$ | rightwards paired arrows | AMS |
| `\rightsquigarrow` | $\rightsquigarrow$ | rightwards squiggle arrow | AMS |
| `\Rrightarrow` | $\Rrightarrow$ | rightwards triple arrow | AMS |
| `\Rsh` | $\Rsh$ | upwards arrow with tip rightwards | AMS |
| `\searrow` | $\searrow$ | south east arrow | TeX |
| `\swarrow` | $\swarrow$ | south west arrow | TeX |
| `\to` | $\to$ | rightwards arrow | TeX |
| `\twoheadleftarrow` | $\twoheadleftarrow$ | leftwards two headed arrow | AMS |
| `\twoheadrightarrow` | $\twoheadrightarrow$ | rightwards two headed arrow | AMS |
| `\Uarr` | $\Uarr$ | upwards double arrow | MediaWiki |
| `\uArr` | $\uArr$ | upwards double arrow | MediaWiki |
| `\uarr` | $\uarr$ | upwards arrow | MediaWiki |
| `\Uparrow` | $\Uparrow$ | upwards double arrow | TeX |
| `\uparrow` | $\uparrow$ | upwards arrow | TeX |
| `\Updownarrow` | $\Updownarrow$ | up down double arrow | TeX |
| `\updownarrow` | $\updownarrow$ | up down arrow | TeX |
| `\upharpoonleft` | $\upharpoonleft$ | upwards harpoon with barb leftwards | AMS |
| `\upharpoonright` | $\upharpoonright$ | upwards harpoon with barb rightwards | AMS |
| `\upuparrows` | $\upuparrows$ | upwards paired arrows | AMS |

### Big operators

| Command | Shows | Name | From |
|---|---|---|---|
| `\bigcap` | $\bigcap$ | n-ary intersection | TeX |
| `\bigcup` | $\bigcup$ | n-ary union | TeX |
| `\bigodot` | $\bigodot$ | n-ary circled dot operator | TeX |
| `\bigoplus` | $\bigoplus$ | n-ary circled plus operator | TeX |
| `\bigotimes` | $\bigotimes$ | n-ary circled times operator | TeX |
| `\bigsqcup` | $\bigsqcup$ | n-ary square union operator | TeX |
| `\biguplus` | $\biguplus$ | n-ary union operator with plus | TeX |
| `\bigvee` | $\bigvee$ | n-ary logical or | TeX |
| `\bigwedge` | $\bigwedge$ | n-ary logical and | TeX |
| `\coprod` | $\coprod$ | n-ary coproduct | TeX |
| `\iiiint` | $\iiiint$ | quadruple integral operator | AMS |
| `\iiint` | $\iiint$ | triple integral | AMS |
| `\iint` | $\iint$ | double integral | AMS |
| `\int` | $\int$ | integral | TeX |
| `\oiiint` | $\oiiint$ | volume integral | MediaWiki |
| `\oiint` | $\oiint$ | surface integral | MediaWiki |
| `\oint` | $\oint$ | contour integral | TeX |
| `\prod` | $\prod$ | n-ary product | TeX |
| `\sum` | $\sum$ | n-ary summation | TeX |

### Delimiters

After `\left` and `\right`, a delimiter grows with its content. After `\big` and the other sized commands, it has a fixed size.

| Command | Shows | Name | From |
|---|---|---|---|
| `\Arrowvert` | $\big\Arrowvert$ | double vertical line | TeX |
| `\arrowvert` | $\big\arrowvert$ | vertical line | TeX |
| `\backslash` | $\big\backslash$ | reverse solidus | TeX |
| `\bracevert` | $\big\bracevert$ | vertical line | TeX |
| `\Downarrow` | $\big\Downarrow$ | downwards double arrow | TeX |
| `\downarrow` | $\big\downarrow$ | downwards arrow | TeX |
| `\gt` | $\big\gt$ | right angle bracket | MediaWiki |
| `\lang` | $\big\lang$ | left angle bracket | MediaWiki |
| `\langle` | $\big\langle$ | left angle bracket | TeX |
| `\lbrace` | $\big\lbrace$ | left curly bracket | TeX |
| `\lbrack` | $\big\lbrack$ | left square bracket | TeX |
| `\lceil` | $\big\lceil$ | left ceiling | TeX |
| `\lfloor` | $\big\lfloor$ | left floor | TeX |
| `\lgroup` | $\big\lgroup$ | left flattened parenthesis | TeX |
| `\llcorner` | $\big\llcorner$ | bottom left corner | AMS |
| `\lmoustache` | $\big\lmoustache$ | upper left or lower right curly bracket section | TeX |
| `\lrcorner` | $\big\lrcorner$ | bottom right corner | AMS |
| `\lt` | $\big\lt$ | left angle bracket | MediaWiki |
| `\lVert` | $\big\lVert$ | double vertical line | AMS |
| `\lvert` | $\big\lvert$ | vertical line | AMS |
| `\rang` | $\big\rang$ | right angle bracket | MediaWiki |
| `\rangle` | $\big\rangle$ | right angle bracket | TeX |
| `\rbrace` | $\big\rbrace$ | right curly bracket | TeX |
| `\rbrack` | $\big\rbrack$ | right square bracket | TeX |
| `\rceil` | $\big\rceil$ | right ceiling | TeX |
| `\rfloor` | $\big\rfloor$ | right floor | TeX |
| `\rgroup` | $\big\rgroup$ | right flattened parenthesis | TeX |
| `\rmoustache` | $\big\rmoustache$ | upper right or lower left curly bracket section | TeX |
| `\rVert` | $\big\rVert$ | double vertical line | AMS |
| `\rvert` | $\big\rvert$ | vertical line | AMS |
| `\ulcorner` | $\big\ulcorner$ | top left corner | AMS |
| `\Uparrow` | $\big\Uparrow$ | upwards double arrow | TeX |
| `\uparrow` | $\big\uparrow$ | upwards arrow | TeX |
| `\Updownarrow` | $\big\Updownarrow$ | up down double arrow | TeX |
| `\updownarrow` | $\big\updownarrow$ | up down arrow | TeX |
| `\urcorner` | $\big\urcorner$ | top right corner | AMS |
| `\Vert` | $\big\Vert$ | double vertical line | TeX |
| `\vert` | $\big\vert$ | vertical line | TeX |
| `\{` | $\big\{$ | left curly bracket | TeX |
| `\}` | $\big\}$ | right curly bracket | TeX |

`\|` is the double bar, the same as `\Vert`. After `\left`, `\right` and the sized commands, `<`, `>`, `\lt` and `\gt` draw angle brackets, as in TeX.

### Punctuation and dots

| Command | Shows | Name | From |
|---|---|---|---|
| `\cdotp` | $\cdotp$ | dot operator | TeX |
| `\cdots` | $\cdots$ | midline horizontal ellipsis | TeX |
| `\colon` | $\colon$ | colon | TeX |
| `\ddots` | $\ddots$ | down right diagonal ellipsis | TeX |
| `\dots` | $\dots$ | horizontal ellipsis | TeX |
| `\dotsb` | $\dotsb$ | midline horizontal ellipsis | AMS |
| `\dotsc` | $\dotsc$ | horizontal ellipsis | AMS |
| `\dotsi` | $\dotsi$ | midline horizontal ellipsis | AMS |
| `\dotsm` | $\dotsm$ | midline horizontal ellipsis | AMS |
| `\ldotp` | $\ldotp$ | full stop | TeX |
| `\ldots` | $\ldots$ | horizontal ellipsis | TeX |
| `\vdots` | $\vdots$ | vertical ellipsis | TeX |

### Other symbols

#### Logic and sets

| Command | Shows | Name | From |
|---|---|---|---|
| `\complement` | $\complement$ | complement | AMS |
| `\empty` | $\empty$ | empty set | MediaWiki |
| `\emptyset` | $\emptyset$ | empty set | TeX |
| `\exist` | $\exist$ | there exists | MediaWiki |
| `\exists` | $\exists$ | there exists | TeX |
| `\forall` | $\forall$ | for all | TeX |
| `\lnot` | $\lnot$ | not sign | TeX |
| `\neg` | $\neg$ | not sign | TeX |
| `\nexists` | $\nexists$ | there does not exist | AMS |
| `\O` | $\O$ | empty set | MediaWiki |
| `\varnothing` | $\varnothing$ | empty set | AMS |

#### Shapes

| Command | Shows | Name | From |
|---|---|---|---|
| `\bigstar` | $\bigstar$ | black star | AMS |
| `\blacklozenge` | $\blacklozenge$ | black lozenge | AMS |
| `\blacksquare` | $\blacksquare$ | black square | AMS |
| `\blacktriangle` | $\blacktriangle$ | black up-pointing triangle | AMS |
| `\blacktriangledown` | $\blacktriangledown$ | black down-pointing triangle | AMS |
| `\Box` | $\Box$ | white square | AMS |
| `\clubs` | $\clubs$ | black club suit | MediaWiki |
| `\clubsuit` | $\clubsuit$ | black club suit | TeX |
| `\Diamond` | $\Diamond$ | lozenge | AMS |
| `\diamonds` | $\diamonds$ | white diamond suit | MediaWiki |
| `\diamondsuit` | $\diamondsuit$ | white diamond suit | TeX |
| `\hearts` | $\hearts$ | white heart suit | MediaWiki |
| `\heartsuit` | $\heartsuit$ | white heart suit | TeX |
| `\lozenge` | $\lozenge$ | lozenge | AMS |
| `\spades` | $\spades$ | black spade suit | MediaWiki |
| `\spadesuit` | $\spadesuit$ | black spade suit | TeX |
| `\square` | $\square$ | white square | AMS |
| `\triangle` | $\triangle$ | white up-pointing triangle | TeX |
| `\triangledown` | $\triangledown$ | white down-pointing triangle | AMS |

#### Miscellaneous

| Command | Shows | Name | From |
|---|---|---|---|
| `\ang` | $\ang$ | angle | MediaWiki |
| `\angle` | $\angle$ | angle | TeX |
| `\Arrowvert` | $\Arrowvert$ | double vertical line | TeX |
| `\arrowvert` | $\arrowvert$ | vertical line | TeX |
| `\backprime` | $\backprime$ | reversed prime | AMS |
| `\backslash` | $\backslash$ | reverse solidus | TeX |
| `\bot` | $\bot$ | up tack | TeX |
| `\bracevert` | $\bracevert$ | vertical line | TeX |
| `\checkmark` | $\checkmark$ | check mark | AMS |
| `\circledS` | $\circledS$ | circled latin capital letter s | AMS |
| `\dag` | $\dag$ | dagger | TeX |
| `\ddag` | $\ddag$ | double dagger | TeX |
| `\degree` | $\degree$ | degree sign | KaTeX |
| `\diagdown` | $\diagdown$ | box drawings light diagonal upper left to lower right | AMS |
| `\diagup` | $\diagup$ | box drawings light diagonal upper right to lower left | AMS |
| `\eth` | $\eth$ | latin small letter eth | AMS |
| `\flat` | $\flat$ | music flat sign | TeX |
| `\infin` | $\infin$ | infinity | MediaWiki |
| `\infty` | $\infty$ | infinity | TeX |
| `\measuredangle` | $\measuredangle$ | measured angle | AMS |
| `\nabla` | $\nabla$ | nabla | TeX |
| `\natural` | $\natural$ | music natural sign | TeX |
| `\P` | $\P$ | pilcrow sign | TeX |
| `\part` | $\part$ | partial differential | MediaWiki |
| `\partial` | $\partial$ | partial differential | TeX |
| `\prime` | $\prime$ | prime | TeX |
| `\S` | $\S$ | section sign | TeX |
| `\sect` | $\sect$ | section sign | MediaWiki |
| `\sharp` | $\sharp$ | music sharp sign | TeX |
| `\sphericalangle` | $\sphericalangle$ | spherical angle | AMS |
| `\surd` | $\surd$ | square root | TeX |
| `\top` | $\top$ | down tack | TeX |
| `\Vert` | $\Vert$ | double vertical line | TeX |
| `\vert` | $\vert$ | vertical line | TeX |

### Escaped characters

The command is a backslash and the character.

| Command | Shows | Name | From |
|---|---|---|---|
| `\#` | $\#$ | number sign | TeX |
| `\$` | $\$$ | dollar sign | TeX |
| `\%` | $\%$ | percent sign | TeX |
| `\&` | $\&$ | ampersand | TeX |
| `\_` | $\_$ | low line | TeX |

### Accents

A narrow accent goes over one letter, a wide one over several.

| Command | Example | Shows | Name | From |
|---|---|---|---|---|
| `\acute` | `\acute{a}` | $\acute{a}$ | combining acute accent | TeX |
| `\bar` | `\bar{a}` | $\bar{a}$ | combining macron | TeX |
| `\breve` | `\breve{a}` | $\breve{a}$ | combining breve | TeX |
| `\check` | `\check{a}` | $\check{a}$ | combining caron | TeX |
| `\ddddot` | `\ddddot{a}` | $\ddddot{a}$ | combining four dots above | AMS |
| `\dddot` | `\dddot{a}` | $\dddot{a}$ | combining three dots above | AMS |
| `\ddot` | `\ddot{a}` | $\ddot{a}$ | combining diaeresis | TeX |
| `\dot` | `\dot{a}` | $\dot{a}$ | combining dot above | TeX |
| `\grave` | `\grave{a}` | $\grave{a}$ | combining grave accent | TeX |
| `\hat` | `\hat{a}` | $\hat{a}$ | combining circumflex accent | TeX |
| `\mathring` | `\mathring{a}` | $\mathring{a}$ | combining ring above | LaTeX |
| `\overleftarrow` | `\overleftarrow{abc}` | $\overleftarrow{abc}$ | combining left arrow above | TeX |
| `\overleftrightarrow` | `\overleftrightarrow{abc}` | $\overleftrightarrow{abc}$ | combining left right arrow above | AMS |
| `\overrightarrow` | `\overrightarrow{abc}` | $\overrightarrow{abc}$ | combining right arrow above | TeX |
| `\tilde` | `\tilde{a}` | $\tilde{a}$ | combining tilde | TeX |
| `\vec` | `\vec{a}` | $\vec{a}$ | combining right arrow above | TeX |
| `\widecheck` | `\widecheck{abc}` | $\widecheck{abc}$ | combining caron | KaTeX |
| `\widehat` | `\widehat{abc}` | $\widehat{abc}$ | combining circumflex accent | TeX |
| `\widetilde` | `\widetilde{abc}` | $\widetilde{abc}$ | combining tilde | TeX |

### Functions

Function names are set upright.

| Command | Shows | Limits | From |
|---|---|---|---|
| `\arccos` | $\arccos x$ | beside | TeX |
| `\arccot` | $\arccot x$ | beside | MediaWiki |
| `\arccsc` | $\arccsc x$ | beside | MediaWiki |
| `\arcsec` | $\arcsec x$ | beside | MediaWiki |
| `\arcsin` | $\arcsin x$ | beside | TeX |
| `\arctan` | $\arctan x$ | beside | TeX |
| `\arg` | $\arg x$ | beside | TeX |
| `\argmax` | $\argmax x$ | below and above in display style | KaTeX |
| `\argmin` | $\argmin x$ | below and above in display style | KaTeX |
| `\cos` | $\cos x$ | beside | TeX |
| `\cosh` | $\cosh x$ | beside | TeX |
| `\cot` | $\cot x$ | beside | TeX |
| `\coth` | $\coth x$ | beside | TeX |
| `\csc` | $\csc x$ | beside | TeX |
| `\csch` | $\csch x$ | beside | Oryx |
| `\deg` | $\deg x$ | beside | TeX |
| `\det` | $\det x$ | below and above in display style | TeX |
| `\dim` | $\dim x$ | beside | TeX |
| `\exp` | $\exp x$ | beside | TeX |
| `\gcd` | $\gcd x$ | below and above in display style | TeX |
| `\hom` | $\hom x$ | beside | TeX |
| `\inf` | $\inf x$ | below and above in display style | TeX |
| `\injlim` | $\injlim x$ | below and above in display style | AMS |
| `\ker` | $\ker x$ | beside | TeX |
| `\lg` | $\lg x$ | beside | TeX |
| `\lim` | $\lim x$ | below and above in display style | TeX |
| `\liminf` | $\liminf x$ | below and above in display style | TeX |
| `\limsup` | $\limsup x$ | below and above in display style | TeX |
| `\ln` | $\ln x$ | beside | TeX |
| `\log` | $\log x$ | beside | TeX |
| `\max` | $\max x$ | below and above in display style | TeX |
| `\min` | $\min x$ | below and above in display style | TeX |
| `\plim` | $\plim x$ | below and above in display style | KaTeX |
| `\Pr` | $\Pr x$ | below and above in display style | TeX |
| `\projlim` | $\projlim x$ | below and above in display style | AMS |
| `\sec` | $\sec x$ | beside | TeX |
| `\sech` | $\sech x$ | beside | Oryx |
| `\sen` | $\sen x$ | beside | MediaWiki |
| `\sgn` | $\sgn x$ | beside | MediaWiki |
| `\sin` | $\sin x$ | beside | TeX |
| `\sinh` | $\sinh x$ | beside | TeX |
| `\sup` | $\sup x$ | below and above in display style | TeX |
| `\tan` | $\tan x$ | beside | TeX |
| `\tanh` | $\tanh x$ | beside | TeX |

### Bounds of an operator

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\limits` | `\int\limits_a^b f` | $\int\limits_a^b f$ | bounds below and above | TeX |
| `\nolimits` | `\sum\nolimits_i x_i` | $\sum\nolimits_i x_i$ | bounds beside | TeX |

### Spacing

The width is in em, the size of the font. A negative width moves the next part to the left.

| Command | Width | Shows | From |
|---|---|---|---|
| `\negthickspace` | -0.28 | $a\negthickspace b$ | LaTeX |
| `\negmedspace` | -0.22 | $a\negmedspace b$ | LaTeX |
| `\!` | -0.17 | $a\!b$ | TeX |
| `\negthinspace` | -0.17 | $a\negthinspace b$ | TeX |
| `\,` | 0.17 | $a\,b$ | TeX |
| `\thinspace` | 0.17 | $a\thinspace b$ | TeX |
| `\:` | 0.22 | $a\:b$ | LaTeX |
| `\medspace` | 0.22 | $a\medspace b$ | LaTeX |
| `\ ` | 0.25 | $a\ b$ | TeX |
| `\nobreakspace` | 0.25 | $a\nobreakspace b$ | LaTeX |
| `\space` | 0.25 | $a\space b$ | MediaWiki |
| `~` | 0.25 | $a~b$ | TeX |
| `\;` | 0.28 | $a\;b$ | TeX |
| `\thickspace` | 0.28 | $a\thickspace b$ | LaTeX |
| `\enspace` | 0.50 | $a\enspace b$ | TeX |
| `\quad` | 1.00 | $a\quad b$ | TeX |
| `\qquad` | 2.00 | $a\qquad b$ | TeX |

### Fractions and binomials

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\frac` | `\frac{a}{b}` | $\frac{a}{b}$ | fraction | LaTeX |
| `\dfrac` | `\dfrac{a}{b}` | $\dfrac{a}{b}$ | fraction at display size | AMS |
| `\tfrac` | `\tfrac{a}{b}` | $\tfrac{a}{b}$ | fraction at text size | AMS |
| `\cfrac` | `\cfrac{1}{1 + x}` | $\cfrac{1}{1 + x}$ | continued fraction | AMS |
| `\binom` | `\binom{n}{k}` | $\binom{n}{k}$ | binomial coefficient | AMS |
| `\dbinom` | `\dbinom{n}{k}` | $\dbinom{n}{k}$ | binomial coefficient at display size | AMS |
| `\tbinom` | `\tbinom{n}{k}` | $\tbinom{n}{k}$ | binomial coefficient at text size | AMS |

### Infix fractions

These go between their two parts, inside braces.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\over` | `{a \over b}` | ${a \over b}$ | fraction, written between its parts | TeX |
| `\atop` | `{a \atop b}` | ${a \atop b}$ | two parts stacked, no bar | TeX |
| `\choose` | `{n \choose k}` | ${n \choose k}$ | binomial coefficient, written between its parts | TeX |
| `\brace` | `{n \brace k}` | ${n \brace k}$ | two parts stacked in braces | TeX |
| `\brack` | `{n \brack k}` | ${n \brack k}$ | two parts stacked in brackets | TeX |

### Roots

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\sqrt` | `\sqrt{x}` | $\sqrt{x}$ | square root | TeX |
| `\sqrt` | `\sqrt[3]{x}` | $\sqrt[3]{x}$ | root of any degree | LaTeX |

### Delimiter sizes

The sized commands set one delimiter at a fixed size. The suffix l, r or m says whether it opens, closes or goes in the middle.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\left` | `\left( \frac{a}{b} \right)` | $\left( \frac{a}{b} \right)$ | the pair grows with its content | TeX |
| `\right` | `\left[ \frac{a}{b} \right]` | $\left[ \frac{a}{b} \right]$ | closes the pair that `\left` opened | TeX |
| `\big` | `\big( x \big)` | $\big( x \big)$ | one delimiter at size 1, the smallest of four | TeX |
| `\bigl` | `\bigl( x \bigr)` | $\bigl( x \bigr)$ | opening delimiter at size 1 | TeX |
| `\bigr` | `\bigl( x \bigr)` | $\bigl( x \bigr)$ | closing delimiter at size 1 | TeX |
| `\bigm` | `a \bigm\vert b` | $a \bigm\vert b$ | middle delimiter at size 1 | TeX |
| `\Big` | `\Big( x \Big)` | $\Big( x \Big)$ | one delimiter at size 2 | TeX |
| `\Bigl` | `\Bigl( x \Bigr)` | $\Bigl( x \Bigr)$ | opening delimiter at size 2 | TeX |
| `\Bigr` | `\Bigl( x \Bigr)` | $\Bigl( x \Bigr)$ | closing delimiter at size 2 | TeX |
| `\Bigm` | `a \Bigm\vert b` | $a \Bigm\vert b$ | middle delimiter at size 2 | TeX |
| `\bigg` | `\bigg( x \bigg)` | $\bigg( x \bigg)$ | one delimiter at size 3 | TeX |
| `\biggl` | `\biggl( x \biggr)` | $\biggl( x \biggr)$ | opening delimiter at size 3 | TeX |
| `\biggr` | `\biggl( x \biggr)` | $\biggl( x \biggr)$ | closing delimiter at size 3 | TeX |
| `\biggm` | `a \biggm\vert b` | $a \biggm\vert b$ | middle delimiter at size 3 | TeX |
| `\Bigg` | `\Bigg( x \Bigg)` | $\Bigg( x \Bigg)$ | one delimiter at size 4, the largest | TeX |
| `\Biggl` | `\Biggl( x \Biggr)` | $\Biggl( x \Biggr)$ | opening delimiter at size 4 | TeX |
| `\Biggr` | `\Biggl( x \Biggr)` | $\Biggl( x \Biggr)$ | closing delimiter at size 4 | TeX |
| `\Biggm` | `a \Biggm\vert b` | $a \Biggm\vert b$ | middle delimiter at size 4 | TeX |

### Text in a formula

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\text` | `x \text{ if } y` | $x \text{ if } y$ | text inside a formula | AMS |
| `\textnormal` | `x \textnormal{ if } y` | $x \textnormal{ if } y$ | another name for `\text` | LaTeX |
| `\textrm` | `x \textrm{ if } y` | $x \textrm{ if } y$ | another name for `\text` | LaTeX |
| `\textup` | `x \textup{ if } y` | $x \textup{ if } y$ | another name for `\text` | LaTeX |
| `\mbox` | `x \mbox{ if } y` | $x \mbox{ if } y$ | another name for `\text` | LaTeX |
| `\hbox` | `x \hbox{ if } y` | $x \hbox{ if } y$ | another name for `\text` | TeX |
| `\operatorname` | `\operatorname{rank} A` | $\operatorname{rank} A$ | a function name set upright | AMS |
| `\operatorname*` | `\operatorname*{argmax}_x f(x)` | $\operatorname*{argmax}_x f(x)$ | a function name with its bounds below and above | AMS |

### Modulo

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\mod` | `a \equiv b \mod n` | $a \equiv b \mod n$ | modulo, with a space before it | AMS |
| `\pmod` | `a \equiv b \pmod n` | $a \equiv b \pmod n$ | modulo, in parentheses | TeX |
| `\bmod` | `a \bmod n` | $a \bmod n$ | modulo, as a binary operator | TeX |

### Lines, braces and stacks

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\overbrace` | `\overbrace{a + b}^{n}` | $\overbrace{a + b}^{n}$ | brace over the content | TeX |
| `\underbrace` | `\underbrace{a + b}_{n}` | $\underbrace{a + b}_{n}$ | brace under the content | TeX |
| `\overline` | `\overline{a + b}` | $\overline{a + b}$ | line over the content | TeX |
| `\underline` | `\underline{a + b}` | $\underline{a + b}$ | line under the content | TeX |
| `\overset` | `\overset{!}{=}` | $\overset{!}{=}$ | one symbol over another | AMS |
| `\underset` | `\underset{n}{=}` | $\underset{n}{=}$ | one symbol under another | AMS |
| `\stackrel` | `\stackrel{\text{def}}{=}` | $\stackrel{\text{def}}{=}$ | one symbol over a relation | LaTeX |

### Extensible arrows

The arrow grows to fit the label above it.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\xrightarrow` | `\xrightarrow{f}` | $\xrightarrow{f}$ | arrow to the right that grows with its label | AMS |
| `\xleftarrow` | `\xleftarrow{f}` | $\xleftarrow{f}$ | arrow to the left that grows with its label | AMS |
| `\xleftrightarrow` | `\xleftrightarrow{f}` | $\xleftrightarrow{f}$ | arrow both ways that grows with its label | MediaWiki |
| `\xRightarrow` | `\xRightarrow{f}` | $\xRightarrow{f}$ | double arrow to the right that grows with its label | KaTeX |
| `\xLeftarrow` | `\xLeftarrow{f}` | $\xLeftarrow{f}$ | double arrow to the left that grows with its label | KaTeX |
| `\xLeftrightarrow` | `\xLeftrightarrow{f}` | $\xLeftrightarrow{f}$ | double arrow both ways that grows with its label | KaTeX |
| `\xmapsto` | `\xmapsto{f}` | $\xmapsto{f}$ | arrow from a bar, as in `\mapsto`, that grows with its label | KaTeX |

### Negation

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\not` | `a \not= b` | $a \not= b$ | slash through the next symbol | TeX |

For these symbols, `\not` gives the negated character, such as ≠ for `\not=`. Before any other symbol, `\not` draws a slash through it.

| Symbol | With `\not` | Shows |
|---|---|---|
| `<` | `\not<` | $\not<$ |
| `=` | `\not=` | $\not=$ |
| `>` | `\not>` | $\not>$ |
| `\leftarrow` | `\not\leftarrow` | $\not\leftarrow$ |
| `\rightarrow` | `\not\rightarrow` | $\not\rightarrow$ |
| `\leftrightarrow` | `\not\leftrightarrow` | $\not\leftrightarrow$ |
| `\Leftarrow` | `\not\Leftarrow` | $\not\Leftarrow$ |
| `\Rightarrow` | `\not\Rightarrow` | $\not\Rightarrow$ |
| `\Leftrightarrow` | `\not\Leftrightarrow` | $\not\Leftrightarrow$ |
| `\exists` | `\not\exists` | $\not\exists$ |
| `\in` | `\not\in` | $\not\in$ |
| `\ni` | `\not\ni` | $\not\ni$ |
| `\mid` | `\not\mid` | $\not\mid$ |
| `\parallel` | `\not\parallel` | $\not\parallel$ |
| `\sim` | `\not\sim` | $\not\sim$ |
| `\simeq` | `\not\simeq` | $\not\simeq$ |
| `\cong` | `\not\cong` | $\not\cong$ |
| `\approx` | `\not\approx` | $\not\approx$ |
| `\asymp` | `\not\asymp` | $\not\asymp$ |
| `\equiv` | `\not\equiv` | $\not\equiv$ |
| `\leq` | `\not\leq` | $\not\leq$ |
| `\geq` | `\not\geq` | $\not\geq$ |
| `\lesssim` | `\not\lesssim` | $\not\lesssim$ |
| `\gtrsim` | `\not\gtrsim` | $\not\gtrsim$ |
| `\lessgtr` | `\not\lessgtr` | $\not\lessgtr$ |
| `\gtrless` | `\not\gtrless` | $\not\gtrless$ |
| `\prec` | `\not\prec` | $\not\prec$ |
| `\succ` | `\not\succ` | $\not\succ$ |
| `\preccurlyeq` | `\not\preccurlyeq` | $\not\preccurlyeq$ |
| `\succcurlyeq` | `\not\succcurlyeq` | $\not\succcurlyeq$ |
| `\subset` | `\not\subset` | $\not\subset$ |
| `\supset` | `\not\supset` | $\not\supset$ |
| `\subseteq` | `\not\subseteq` | $\not\subseteq$ |
| `\supseteq` | `\not\supseteq` | $\not\supseteq$ |
| `\sqsubseteq` | `\not\sqsubseteq` | $\not\sqsubseteq$ |
| `\sqsupseteq` | `\not\sqsupseteq` | $\not\sqsupseteq$ |
| `\vdash` | `\not\vdash` | $\not\vdash$ |
| `\models` | `\not\models` | $\not\models$ |
| `\Vdash` | `\not\Vdash` | $\not\Vdash$ |
| `\vartriangleleft` | `\not\vartriangleleft` | $\not\vartriangleleft$ |
| `\vartriangleright` | `\not\vartriangleright` | $\not\vartriangleright$ |
| `\trianglelefteq` | `\not\trianglelefteq` | $\not\trianglelefteq$ |
| `\trianglerighteq` | `\not\trianglerighteq` | $\not\trianglerighteq$ |

### Phantoms

A phantom takes the space of its content and draws nothing.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\phantom` | `a \phantom{b} c` | $a \phantom{b} c$ | space of the content, nothing drawn | TeX |
| `\hphantom` | `a \hphantom{b} c` | $a \hphantom{b} c$ | width of the content, nothing drawn | TeX |
| `\vphantom` | `\left( \vphantom{\frac{a}{b}} x \right)` | $\left( \vphantom{\frac{a}{b}} x \right)$ | height and depth of the content, no width | TeX |

### Color

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\color` | `{\color{red} x}` | ${\color{red} x}$ | color for the rest of the group | LaTeX |
| `\textcolor` | `\textcolor{red}{x}` | $\textcolor{red}{x}$ | color for the argument | LaTeX |

The color names, as used in `\textcolor{red}{x}`. A color can also be written as `#rgb` or `#rrggbb`, as in `\textcolor{#f80}{x}`.

| Name | Shows |
|---|---|
| `black` | $\textcolor{black}{x}$ |
| `blue` | $\textcolor{blue}{x}$ |
| `brown` | $\textcolor{brown}{x}$ |
| `cyan` | $\textcolor{cyan}{x}$ |
| `darkgray` | $\textcolor{darkgray}{x}$ |
| `gray` | $\textcolor{gray}{x}$ |
| `green` | $\textcolor{green}{x}$ |
| `lightgray` | $\textcolor{lightgray}{x}$ |
| `lime` | $\textcolor{lime}{x}$ |
| `magenta` | $\textcolor{magenta}{x}$ |
| `olive` | $\textcolor{olive}{x}$ |
| `orange` | $\textcolor{orange}{x}$ |
| `pink` | $\textcolor{pink}{x}$ |
| `purple` | $\textcolor{purple}{x}$ |
| `red` | $\textcolor{red}{x}$ |
| `teal` | $\textcolor{teal}{x}$ |
| `violet` | $\textcolor{violet}{x}$ |
| `white` | $\textcolor{white}{x}$ |
| `yellow` | $\textcolor{yellow}{x}$ |

### Classes

A class sets the spacing around its content, as if it were a symbol of that kind.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\mathord` | `a \mathord{+} b` | $a \mathord{+} b$ | treated as an ordinary symbol | TeX |
| `\mathop` | `\mathop{M} x` | $\mathop{M} x$ | treated as a big operator | TeX |
| `\mathbin` | `a \mathbin{R} b` | $a \mathbin{R} b$ | treated as a binary operator | TeX |
| `\mathrel` | `a \mathrel{R} b` | $a \mathrel{R} b$ | treated as a relation | TeX |
| `\mathopen` | `\mathopen{R} x` | $\mathopen{R} x$ | treated as an opening symbol | TeX |
| `\mathclose` | `x \mathclose{R}` | $x \mathclose{R}$ | treated as a closing symbol | TeX |
| `\mathpunct` | `a \mathpunct{R} b` | $a \mathpunct{R} b$ | treated as punctuation | TeX |
| `\mathinner` | `a \mathinner{R} b` | $a \mathinner{R} b$ | treated as an inner formula, like a fraction | TeX |

### Alphabets

These change the letters of the argument.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\mathrm` | `\mathrm{d}x` | $\mathrm{d}x$ | upright letters | LaTeX |
| `\mathbb` | `\mathbb{R}` | $\mathbb{R}$ | double-struck letters | AMS |
| `\Bbb` | `\Bbb{R}` | $\Bbb{R}$ | double-struck letters, old name | AMS |
| `\mathbf` | `\mathbf{x}` | $\mathbf{x}$ | bold upright letters | LaTeX |
| `\bold` | `\bold{x}` | $\bold{x}$ | bold upright letters, old name of `\mathbf` | AMS |
| `\bm` | `\bm{x}` | $\bm{x}$ | bold letters and symbols, the same as `\boldsymbol` | LaTeX |
| `\boldsymbol` | `\boldsymbol{x}` | $\boldsymbol{x}$ | bold letters and symbols | LaTeX |
| `\mathcal` | `\mathcal{L}` | $\mathcal{L}$ | calligraphic letters | LaTeX |
| `\mathscr` | `\mathscr{L}` | $\mathscr{L}$ | script letters, the same shapes as `\mathcal` with this font | MediaWiki |
| `\scr` | `\scr{L}` | $\scr{L}$ | script letters, short name | MediaWiki |
| `\mathfrak` | `\mathfrak{g}` | $\mathfrak{g}$ | Fraktur letters | AMS |
| `\frak` | `\frak{g}` | $\frak{g}$ | Fraktur letters, old name | AMS |
| `\mathit` | `\mathit{ab}` | $\mathit{ab}$ | italic letters | LaTeX |
| `\mathnormal` | `\mathnormal{ab}` | $\mathnormal{ab}$ | the normal math letters | LaTeX |
| `\mathsf` | `\mathsf{ab}` | $\mathsf{ab}$ | sans serif letters | LaTeX |
| `\mathtt` | `\mathtt{ab}` | $\mathtt{ab}$ | typewriter letters | LaTeX |

### Font switches

These change the font for the rest of the group.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\bf` | `{\bf ab}` | ${\bf ab}$ | bold, for the rest of the group | TeX |
| `\it` | `{\it ab}` | ${\it ab}$ | italic, for the rest of the group | TeX |
| `\rm` | `{\rm ab}` | ${\rm ab}$ | upright, for the rest of the group | TeX |
| `\sf` | `{\sf ab}` | ${\sf ab}$ | sans serif, for the rest of the group | MediaWiki |
| `\tt` | `{\tt ab}` | ${\tt ab}$ | typewriter, for the rest of the group | TeX |
| `\cal` | `{\cal AB}` | ${\cal AB}$ | calligraphic, for the rest of the group | TeX |

### Style switches

These change the style, and so the size, until the end of the group.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\displaystyle` | `{\displaystyle \sum_i x_i}` | ${\displaystyle \sum_i x_i}$ | full size, as on a line of its own | TeX |
| `\textstyle` | `{\textstyle \sum_i x_i}` | ${\textstyle \sum_i x_i}$ | the size of a formula in a sentence | TeX |
| `\scriptstyle` | `{\scriptstyle \sum_i x_i}` | ${\scriptstyle \sum_i x_i}$ | the size of an exponent | TeX |
| `\scriptscriptstyle` | `{\scriptscriptstyle \sum_i x_i}` | ${\scriptscriptstyle \sum_i x_i}$ | the size of an exponent's exponent | TeX |

### Macros

`\newcommand` defines a command that works until the end of the formula.

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\newcommand` | `\newcommand{\half}{\frac{1}{2}} \half` | $\newcommand{\half}{\frac{1}{2}} \half$ | defines a new command | LaTeX |
| `\renewcommand` | `\newcommand{\half}{1} \renewcommand{\half}{\frac{1}{2}} \half` | $\newcommand{\half}{1} \renewcommand{\half}{\frac{1}{2}} \half$ | defines a command again | LaTeX |

### Environments

| Command | Example | Shows | Meaning | From |
|---|---|---|---|---|
| `\begin` | `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` | $\begin{pmatrix} a & b \\ c & d \end{pmatrix}$ | opens an environment | LaTeX |
| `\end` | `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` | $\begin{pmatrix} a & b \\ c & d \end{pmatrix}$ | closes the environment | LaTeX |
| `\begin{aligned}` | `\begin{aligned} a &= b \\ c &= d \end{aligned}` | $\begin{aligned} a &= b \\ c &= d \end{aligned}$ | lines aligned at their `&` marks | AMS |
| `\begin{array}` | `\begin{array}{cc} a & b \\ c & d \end{array}` | $\begin{array}{cc} a & b \\ c & d \end{array}$ | table with chosen column alignment | LaTeX |
| `\begin{Bmatrix}` | `\begin{Bmatrix} a & b \\ c & d \end{Bmatrix}` | $\begin{Bmatrix} a & b \\ c & d \end{Bmatrix}$ | matrix in braces | AMS |
| `\begin{bmatrix}` | `\begin{bmatrix} a & b \\ c & d \end{bmatrix}` | $\begin{bmatrix} a & b \\ c & d \end{bmatrix}$ | matrix in brackets | AMS |
| `\begin{cases}` | `\begin{cases} x & x > 0 \\ -x & x < 0 \end{cases}` | $\begin{cases} x & x > 0 \\ -x & x < 0 \end{cases}$ | cases, with a brace on the left | AMS |
| `\begin{matrix}` | `\begin{matrix} a & b \\ c & d \end{matrix}` | $\begin{matrix} a & b \\ c & d \end{matrix}$ | matrix without delimiters | AMS |
| `\begin{pmatrix}` | `\begin{pmatrix} a & b \\ c & d \end{pmatrix}` | $\begin{pmatrix} a & b \\ c & d \end{pmatrix}$ | matrix in parentheses | AMS |
| `\begin{smallmatrix}` | `\begin{smallmatrix} a & b \\ c & d \end{smallmatrix}` | $\begin{smallmatrix} a & b \\ c & d \end{smallmatrix}$ | small matrix for use in a line of text | AMS |
| `\begin{Vmatrix}` | `\begin{Vmatrix} a & b \\ c & d \end{Vmatrix}` | $\begin{Vmatrix} a & b \\ c & d \end{Vmatrix}$ | matrix between double bars | AMS |
| `\begin{vmatrix}` | `\begin{vmatrix} a & b \\ c & d \end{vmatrix}` | $\begin{vmatrix} a & b \\ c & d \end{vmatrix}$ | matrix between single bars | AMS |

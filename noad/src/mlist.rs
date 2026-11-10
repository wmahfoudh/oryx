//! The math list: what the parser builds and layout consumes.
//!
//! Knuth's taxonomy. Every element is a noad; a Task 59 noad is an atom
//! carrying a nucleus and optional scripts. The atom classes drive the
//! inter-atom spacing matrix in layout. Later construct noads (fractions,
//! radicals, operators with limits, tables) join this enum as they land.

use std::ops::Range;

/// TeX's atom classes, the input to the spacing matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomClass {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

/// The four TeX styles. Cramped variants are tracked inside the layout.
/// A construct that forces its own style (`\dfrac`, `\tfrac`) carries
/// one; the layout takes the style around it otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// A nucleus, superscript, or subscript content. Constructs are nuclei,
/// so scripts, spacing, and demotion see every noad as an atom.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// A single symbol, the source character or a command's resolved
    /// codepoint. Style-dependent remapping (math italic, alphabets) is
    /// layout's concern.
    Symbol(char),
    /// A braced group.
    List(MathList),
    /// A fraction or barless stack; `\binom` is a stack inside delimiters.
    Fraction {
        numerator: MathList,
        denominator: MathList,
        /// False for `\binom` and `\atop`: the stack constants take over.
        bar: bool,
        /// The style the fraction is laid out in whatever the style
        /// around it: `\tfrac` and `\tbinom` force text style, `\dfrac`,
        /// `\dbinom` and `\cfrac` display style. `\frac` carries none.
        style: Option<MathStyle>,
        /// `\cfrac`: each part stands on a strut, so that nested
        /// continued fractions keep even rows.
        continued: bool,
    },
    /// A radical with an optional degree.
    Radical {
        radicand: MathList,
        degree: Option<MathList>,
    },
    /// A `\left...\right` group with its stretchy delimiters. A `.` in the
    /// source means no delimiter on that side.
    LeftRight {
        open: Option<char>,
        close: Option<char>,
        body: MathList,
    },
    /// A delimiter at one of TeX's four fixed sizes, `\big` to `\Bigg`
    /// (the size 1 to 4), with the class of the word's form on the atom:
    /// Ord bare, Open for `l`, Close for `r`, Rel for `m`. `None` is the
    /// dot form, TeX's invisible box of the size.
    Sized { ch: Option<char>, size: u8 },
    /// `\overline` and `\underline`: a rule over or under the base.
    Line { base: MathList, under: bool },
    /// `\overbrace` and `\underbrace`: a brace stretched over or under the
    /// base; the atom is an operator with limits, so its scripts stack
    /// beyond the brace.
    Brace { base: MathList, under: bool },
    /// `\xrightarrow` and its kin: an arrow stretched past the texts set
    /// above and below it.
    Arrow {
        ch: char,
        above: MathList,
        below: MathList,
    },
    /// `\phantom`, `\vphantom`, `\hphantom`: the size of the body with no
    /// ink, the width kept or zero, the height and depth kept or zero.
    Phantom {
        body: MathList,
        width: bool,
        height: bool,
    },
    /// `\textcolor{c}{...}`: the body in a color.
    Colored { color: [u8; 3], body: MathList },
    /// `\not` before a symbol with no negated form of its own: the
    /// combining long solidus drawn over it; a relation, as `\not` is.
    Negated { base: MathList },
    /// An accented base: the combining accent character, whether wide
    /// forms may stretch horizontally, and the accented list.
    Accent {
        accent: char,
        stretch: bool,
        base: MathList,
    },
    /// An environment's grid: rows of cells, one alignment per column
    /// with the pattern repeating past its end, the matrix family's
    /// interior. `small` sets the cells at script size.
    Table {
        rows: Vec<Vec<MathList>>,
        align: Vec<ColAlign>,
        gaps: TableGaps,
        small: bool,
    },
    /// Upright text: `\text{...}` verbatim, and operator names like
    /// `\sin`, rendered without the italic remap.
    Text(String),
    /// An explicit space in ems of the current style, the `\,` family.
    /// Transparent to spacing and demotion, TeX's kern item.
    Kern(f32),
    /// TeX the engine does not understand, carried verbatim for the host to
    /// render as a literal. The quiet fallback.
    Literal(String),
    /// An empty nucleus, TeX's implicit `{}`.
    Empty,
}

/// A table column's alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColAlign {
    Left,
    Center,
    Right,
}

/// A table's inter-column gap rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TableGaps {
    /// One uniform gap between adjacent columns, in ems of the cell size.
    Em(f32),
    /// amsmath's `aligned`: nothing inside an (r, l) pair, one em
    /// between pairs, so derivations touch their relations.
    Pairs,
}

/// How an operator takes its scripts: TeX's `\limits` machinery. Default
/// resolves by operator and style at layout: sum-class operators take
/// limits in display style, integrals stay beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Limits {
    #[default]
    Default,
    Limits,
    NoLimits,
}

/// An atom: nucleus with optional scripts, classed for spacing.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub class: AtomClass,
    pub nucleus: Field,
    pub sup: Option<MathList>,
    pub sub: Option<MathList>,
    /// Script placement for operator atoms.
    pub limits: Limits,
    /// Byte range of the whole atom, scripts included.
    pub span: Range<usize>,
    /// Byte range of the nucleus alone, what its glyph stamps carry.
    pub nucleus_span: Range<usize>,
}

/// A math list element.
#[derive(Debug, Clone, PartialEq)]
pub enum Noad {
    Atom(Atom),
    /// TeX's style noad, `\displaystyle` and its kin: the style of the
    /// rest of the list, uncramped. It is no atom: the binary rules and
    /// the scripts see through it, and the spacing after it is read in
    /// the new style.
    Style(MathStyle),
    /// `\color{c}`: the rest of the list in the color, the glyphs and the
    /// rules, an inner color winning. No atom either.
    Color([u8; 3]),
}

/// A parsed math list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathList(pub Vec<Noad>);

impl MathList {
    pub fn atoms(&self) -> impl Iterator<Item = &Atom> {
        self.0.iter().filter_map(|n| match n {
            Noad::Atom(a) => Some(a),
            Noad::Style(_) | Noad::Color(_) => None,
        })
    }

    /// The TeX the parser did not understand: the text of every
    /// `Field::Literal` through every field and script, in the order of
    /// the list, a superscript before a subscript. Empty means the whole
    /// formula typesets.
    pub fn literals(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_literals(&mut out, false);
        out
    }

    /// What the page shows as written: the literals, and with them the
    /// texts that hold a backslash (`\text{\alpha}`), which a text shows
    /// as written. The probe of a file and the test of the math page
    /// read this.
    pub fn shown_as_written(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_literals(&mut out, true);
        out
    }

    fn collect_literals(&self, out: &mut Vec<String>, texts: bool) {
        for atom in self.atoms() {
            // Exhaustive on purpose: a field added later must say where
            // its literals are.
            match &atom.nucleus {
                Field::Literal(text) => out.push(text.clone()),
                Field::List(list) => list.collect_literals(out, texts),
                Field::Fraction {
                    numerator,
                    denominator,
                    ..
                } => {
                    numerator.collect_literals(out, texts);
                    denominator.collect_literals(out, texts);
                }
                Field::Radical { radicand, degree } => {
                    if let Some(degree) = degree {
                        degree.collect_literals(out, texts);
                    }
                    radicand.collect_literals(out, texts);
                }
                Field::LeftRight { body, .. }
                | Field::Phantom { body, .. }
                | Field::Colored { body, .. } => body.collect_literals(out, texts),
                Field::Line { base, .. } | Field::Brace { base, .. } | Field::Negated { base } => {
                    base.collect_literals(out, texts)
                }
                Field::Arrow { above, below, .. } => {
                    above.collect_literals(out, texts);
                    below.collect_literals(out, texts);
                }
                Field::Accent { base, .. } => base.collect_literals(out, texts),
                Field::Table { rows, .. } => {
                    for row in rows {
                        for cell in row {
                            cell.collect_literals(out, texts);
                        }
                    }
                }
                // A backslash left in a text is shown: `\text{\alpha}`,
                // `\text{G\"odel}`, where TeX would typeset the letter.
                Field::Text(text) if texts && text.contains('\\') => out.push(text.clone()),
                Field::Symbol(_)
                | Field::Sized { .. }
                | Field::Text(_)
                | Field::Kern(_)
                | Field::Empty => {}
            }
            if let Some(list) = &atom.sup {
                list.collect_literals(out, texts);
            }
            if let Some(list) = &atom.sub {
                list.collect_literals(out, texts);
            }
        }
    }
}

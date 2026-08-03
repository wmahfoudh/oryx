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
    /// An accented base: the combining accent character, whether wide
    /// forms may stretch horizontally, and the accented list.
    Accent {
        accent: char,
        stretch: bool,
        base: MathList,
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
}

/// A parsed math list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathList(pub Vec<Noad>);

impl MathList {
    pub fn atoms(&self) -> impl Iterator<Item = &Atom> {
        self.0.iter().map(|n| match n {
            Noad::Atom(a) => a,
        })
    }
}

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

/// A nucleus, superscript, or subscript content.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// A single symbol, the source character or a command's resolved
    /// codepoint. Style-dependent remapping (math italic, alphabets) is
    /// layout's concern.
    Symbol(char),
    /// A braced group.
    List(MathList),
    /// TeX the engine does not understand, carried verbatim for the host to
    /// render as a literal. The quiet fallback.
    Literal(String),
    /// An empty nucleus, TeX's implicit `{}`.
    Empty,
}

/// An atom: nucleus with optional scripts, classed for spacing.
#[derive(Debug, Clone, PartialEq)]
pub struct Atom {
    pub class: AtomClass,
    pub nucleus: Field,
    pub sup: Option<MathList>,
    pub sub: Option<MathList>,
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

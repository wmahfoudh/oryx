//! TeX math typesetting.
//!
//! noad parses a TeX math string into a math list and lays it out by the
//! rules of the TeXbook's Appendix G, driven by the OpenType MATH metrics of
//! a font the host supplies. The output is renderer-agnostic geometry:
//! positioned glyphs, rules, and box metrics, each element stamped with the
//! byte range of the TeX source it came from.
//!
//! The crate name is Knuth's term for the elements of a math list, atoms
//! carrying a nucleus, a superscript, and a subscript.

pub mod font;
pub mod layout;
pub mod mlist;
pub mod parse;
pub mod token;

pub use font::MathFont;

//! Appendix G layout: a math list to flat geometry.
//!
//! One recursive walk in the current style, emitting positioned glyphs,
//! rules, and literal boxes relative to the expression's baseline origin.
//! Coordinates follow the host convention: x grows right, y grows down,
//! the baseline at y zero, so a superscript sits at negative y. `ascent`
//! and `descent` are positive extents above and below the baseline.
//!
//! `size` is the em size of text style in pixels; display and text render
//! at it, script and scriptscript at the font's percentage scale-downs.

use crate::font::MathFont;
use crate::mlist::{Atom, AtomClass, Field, MathList, Noad};
use std::ops::Range;

/// The four TeX styles. Cramped variants are tracked internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// A glyph at its position, in the math font at `size` pixels per em.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub glyph: crate::font::GlyphId,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// Byte range of the TeX source this glyph renders.
    pub source: Range<usize>,
}

/// A filled rectangle: fraction bars, radical strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A box reserved for literal TeX the host renders in its own face.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub width: f32,
    pub source: Range<usize>,
}

/// The laid-out expression.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    pub glyphs: Vec<PositionedGlyph>,
    pub rules: Vec<Rule>,
    pub literals: Vec<Literal>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Parses and lays out a TeX math string.
pub fn layout(tex: &str, style: MathStyle, size: f32, font: &dyn MathFont) -> MathLayout {
    let list = crate::parse::parse(tex);
    Ctx { font, base: size }.hlist(&list, style, false)
}

/// TeX's inter-atom spacing matrix, rows the left class, columns the right,
/// in the class order of [`AtomClass`]. Magnitudes are eighteenths of an em:
/// 3 thin, 4 medium, 5 thick. Negative entries are the TeXbook's
/// parenthesized ones, omitted in script styles. Pairs demotion makes
/// impossible hold zero.
#[rustfmt::skip]
const SPACING: [[i8; 8]; 8] = [
    // Ord Op  Bin Rel Open Close Punct Inner
    [0,  3, -4, -5,  0,  0,  0, -3], // Ord
    [3,  3,  0, -5,  0,  0,  0, -3], // Op
    [-4, -4,  0,  0, -4,  0,  0, -4], // Bin
    [-5, -5,  0,  0, -5,  0,  0, -5], // Rel
    [0,  0,  0,  0,  0,  0,  0,  0], // Open
    [0,  3, -4, -5,  0,  0,  0, -3], // Close
    [-3, -3,  0, -3, -3, -3, -3, -3], // Punct
    [-3,  3, -4, -5, -3,  0, -3, -3], // Inner
];

/// The classic italic remapping: letters render from the Mathematical
/// Alphanumeric block, the way TeX sets variables. Latin h is the one hole
/// in the block, U+210E Planck constant. Uppercase Greek stays upright.
fn math_italic(c: char) -> char {
    let mapped = match c {
        'h' => 0x210E,
        'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
        'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
        '\u{03B1}'..='\u{03C9}' => 0x1D6FC + (c as u32 - 0x03B1),
        _ => c as u32,
    };
    char::from_u32(mapped).unwrap_or(c)
}

struct Ctx<'a> {
    font: &'a dyn MathFont,
    /// Text-style em size in pixels.
    base: f32,
}

/// An atom or list laid out relative to its own baseline origin, plus the
/// italic correction of its last nucleus glyph for script attachment.
struct LaidBox {
    geom: MathLayout,
    italic: f32,
}

impl Ctx<'_> {
    fn style_scale(&self, style: MathStyle) -> f32 {
        let c = self.font.constants();
        match style {
            MathStyle::Display | MathStyle::Text => 1.0,
            MathStyle::Script => c.script_percent_scale_down / 100.0,
            MathStyle::ScriptScript => c.script_script_percent_scale_down / 100.0,
        }
    }

    /// Em size at the style, pixels.
    fn em(&self, style: MathStyle) -> f32 {
        self.base * self.style_scale(style)
    }

    /// Design units to pixels at the style.
    fn unit(&self, style: MathStyle) -> f32 {
        self.em(style) / self.font.units_per_em()
    }

    fn script_style(style: MathStyle) -> MathStyle {
        match style {
            MathStyle::Display | MathStyle::Text => MathStyle::Script,
            MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
        }
    }

    fn space(&self, left: AtomClass, right: AtomClass, style: MathStyle) -> f32 {
        let entry = SPACING[left as usize][right as usize];
        let script = matches!(style, MathStyle::Script | MathStyle::ScriptScript);
        if entry == 0 || (entry < 0 && script) {
            return 0.0;
        }
        f32::from(entry.abs()) / 18.0 * self.em(style)
    }

    fn hlist(&self, list: &MathList, style: MathStyle, cramped: bool) -> MathLayout {
        let mut out = MathLayout::default();
        let mut cursor = 0.0;
        let mut prev: Option<AtomClass> = None;
        for noad in &list.0 {
            let Noad::Atom(atom) = noad;
            if let Some(p) = prev {
                cursor += self.space(p, atom.class, style);
            }
            let laid = self.atom_box(atom, style, cramped);
            let width = laid.geom.width;
            merge(&mut out, laid.geom, cursor, 0.0);
            cursor += width;
            prev = Some(atom.class);
        }
        out.width = cursor;
        out
    }

    fn nucleus_box(&self, atom: &Atom, style: MathStyle, cramped: bool) -> LaidBox {
        match &atom.nucleus {
            Field::Symbol(c) => {
                let mapped = math_italic(*c);
                let Some(glyph) = self.font.glyph(mapped) else {
                    // A codepoint the font cannot draw degrades to a literal
                    // box the host renders, same as an unknown command.
                    return self.literal_box(&c.to_string(), atom, style);
                };
                let u = self.unit(style);
                let bounds = self.font.bounds(glyph);
                let geom = MathLayout {
                    glyphs: vec![PositionedGlyph {
                        glyph,
                        x: 0.0,
                        y: 0.0,
                        size: self.em(style),
                        source: atom.nucleus_span.clone(),
                    }],
                    rules: Vec::new(),
                    literals: Vec::new(),
                    width: self.font.advance(glyph) * u,
                    ascent: (bounds.y_max * u).max(0.0),
                    descent: (-bounds.y_min * u).max(0.0),
                };
                LaidBox {
                    geom,
                    italic: self.font.italic_correction(glyph) * u,
                }
            }
            Field::List(inner) => LaidBox {
                geom: self.hlist(inner, style, cramped),
                italic: 0.0,
            },
            Field::Literal(text) => self.literal_box(text, atom, style),
            Field::Empty => LaidBox {
                geom: MathLayout::default(),
                italic: 0.0,
            },
        }
    }

    fn literal_box(&self, text: &str, atom: &Atom, style: MathStyle) -> LaidBox {
        let em = self.em(style);
        let width = self.font.measure_literal(text, em);
        let geom = MathLayout {
            glyphs: Vec::new(),
            rules: Vec::new(),
            literals: vec![Literal {
                text: text.to_string(),
                x: 0.0,
                y: 0.0,
                size: em,
                width,
                source: atom.nucleus_span.clone(),
            }],
            width,
            ascent: em * 0.8,
            descent: em * 0.2,
        };
        LaidBox { geom, italic: 0.0 }
    }

    /// Appendix G's script attachment: shifts from the constants with the
    /// collision clauses, italic correction on the superscript side, the
    /// script space closing the atom.
    fn atom_box(&self, atom: &Atom, style: MathStyle, cramped: bool) -> LaidBox {
        let nucleus = self.nucleus_box(atom, style, cramped);
        if atom.sup.is_none() && atom.sub.is_none() {
            return nucleus;
        }

        let c = self.font.constants();
        let u = self.unit(style);
        let sub_style = Self::script_style(style);
        let mut geom = nucleus.geom;
        let nucleus_width = geom.width;

        let sup = atom
            .sup
            .as_ref()
            .map(|list| self.hlist(list, sub_style, cramped));
        let sub = atom
            .sub
            .as_ref()
            .map(|list| self.hlist(list, sub_style, true));

        // Shifts are positive: sup up, sub down.
        let mut sup_shift = 0.0f32;
        if let Some(s) = &sup {
            let plain = if cramped {
                c.superscript_shift_up_cramped
            } else {
                c.superscript_shift_up
            } * u;
            sup_shift = plain.max(s.descent + c.superscript_bottom_min * u);
        }
        let mut sub_shift = 0.0f32;
        if let Some(s) = &sub {
            sub_shift = (c.subscript_shift_down * u).max(s.ascent - c.subscript_top_max * u);
        }
        if let (Some(sup_box), Some(sub_box)) = (&sup, &sub) {
            let gap = (sup_shift - sup_box.descent) - (sub_box.ascent - sub_shift);
            let deficit = c.sub_superscript_gap_min * u - gap;
            if deficit > 0.0 {
                sub_shift += deficit;
            }
        }

        let mut end = nucleus_width;
        if let Some(s) = sup {
            let x = nucleus_width + nucleus.italic;
            let w = s.width;
            merge(&mut geom, s, x, -sup_shift);
            end = end.max(x + w);
        }
        if let Some(s) = sub {
            let w = s.width;
            merge(&mut geom, s, nucleus_width, sub_shift);
            end = end.max(nucleus_width + w);
        }
        geom.width = end + c.space_after_script * u;
        LaidBox { geom, italic: 0.0 }
    }
}

/// Translates `src` by (dx, dy) into `dst`, extending the vertical extents.
/// Widths stay the caller's business.
fn merge(dst: &mut MathLayout, src: MathLayout, dx: f32, dy: f32) {
    for mut g in src.glyphs {
        g.x += dx;
        g.y += dy;
        dst.glyphs.push(g);
    }
    for mut r in src.rules {
        r.x += dx;
        r.y += dy;
        dst.rules.push(r);
    }
    for mut l in src.literals {
        l.x += dx;
        l.y += dy;
        dst.literals.push(l);
    }
    dst.ascent = dst.ascent.max(src.ascent - dy);
    dst.descent = dst.descent.max(src.descent + dy);
}

#[cfg(all(test, feature = "ttf"))]
mod tests {
    use super::*;
    use crate::font::TtfMathFont;

    const STIX: &[u8] = include_bytes!("../fixtures/STIXTwoMath-Regular.otf");
    const SIZE: f32 = 20.0;
    const EPS: f32 = 0.01;

    fn font() -> TtfMathFont<'static> {
        TtfMathFont::from_bytes(STIX).unwrap()
    }

    fn lay(tex: &str) -> MathLayout {
        layout(tex, MathStyle::Text, SIZE, &font())
    }

    fn scale() -> f32 {
        use crate::MathFont as _;
        SIZE / font().units_per_em()
    }

    fn glyph_of(c: char) -> crate::font::GlyphId {
        use crate::MathFont as _;
        font().glyph(c).unwrap()
    }

    fn adv(c: char) -> f32 {
        use crate::MathFont as _;
        font().advance(glyph_of(c)) * scale()
    }

    #[test]
    fn letters_map_to_math_italic_and_digits_stay_upright() {
        let l = lay("x2");
        assert_eq!(l.glyphs.len(), 2);
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{1D465}'));
        assert_eq!(l.glyphs[1].glyph, glyph_of('2'));
        assert_eq!(l.glyphs[0].y, 0.0);
        assert_eq!(l.glyphs[0].size, SIZE);
        assert!(l.ascent > 0.0 && l.width > 0.0);
    }

    #[test]
    fn planck_h_and_greek_take_their_italic_codepoints() {
        let l = lay("h");
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{210E}'));
        let l = lay("\\alpha\\Gamma");
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{1D6FC}'));
        assert_eq!(l.glyphs[1].glyph, glyph_of('\u{0393}'));
    }

    #[test]
    fn spacing_matrix_places_medium_and_thick_spaces() {
        // Ord Bin Ord: medium spaces, 4/18 em.
        let l = lay("a+b");
        let med = 4.0 / 18.0 * SIZE;
        let after_a = adv('\u{1D44E}');
        assert!((l.glyphs[1].x - (after_a + med)).abs() < EPS);
        // Ord Rel Ord: thick spaces, 5/18 em.
        let l = lay("a=b");
        let thick = 5.0 / 18.0 * SIZE;
        assert!((l.glyphs[1].x - (after_a + thick)).abs() < EPS);
        // Ord Open: no space.
        let l = lay("f(a)");
        let after_f = adv('\u{1D453}');
        assert!((l.glyphs[1].x - after_f).abs() < EPS);
    }

    #[test]
    fn script_styles_drop_the_parenthesized_spaces() {
        let l = layout("a+b", MathStyle::Script, SIZE, &font());
        use crate::MathFont as _;
        let c = font().constants();
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let after_a = font().advance(glyph_of('\u{1D44E}')) * script_scale;
        assert!((l.glyphs[1].x - after_a).abs() < EPS);
        assert!((l.glyphs[0].size - SIZE * c.script_percent_scale_down / 100.0).abs() < EPS);
    }

    #[test]
    fn superscript_rises_by_the_constant_at_script_size() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("x^2");
        assert_eq!(l.glyphs.len(), 2);
        let sup = &l.glyphs[1];
        assert!((sup.size - SIZE * c.script_percent_scale_down / 100.0).abs() < EPS);
        // The digit 2 has no descent to speak of, so the plain shift wins.
        let expected_y = -c.superscript_shift_up * scale();
        assert!(
            (sup.y - expected_y).abs() < EPS,
            "sup y {} vs {}",
            sup.y,
            expected_y
        );
        assert!(l.ascent > SIZE * 0.5);
    }

    #[test]
    fn deep_superscript_honors_the_bottom_minimum() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        // g descends; its bottom must clear SuperscriptBottomMin.
        let l = lay("x^g");
        let sup = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let g_descent = -f.bounds(glyph_of('\u{1D454}')).y_min * script_scale;
        let plain = c.superscript_shift_up * scale();
        let clause = g_descent + c.superscript_bottom_min * scale();
        let expected_y = -plain.max(clause);
        assert!(
            (sup.y - expected_y).abs() < EPS,
            "sup y {} vs {}",
            sup.y,
            expected_y
        );
    }

    #[test]
    fn subscript_drops_by_the_constant() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("a_i");
        let sub = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let i_ascent = f.bounds(glyph_of('\u{1D456}')).y_max * script_scale;
        let plain = c.subscript_shift_down * scale();
        let clause = i_ascent - c.subscript_top_max * scale();
        let expected_y = plain.max(clause);
        assert!(
            (sub.y - expected_y).abs() < EPS,
            "sub y {} vs {}",
            sub.y,
            expected_y
        );
        assert!(l.descent > 0.0);
    }

    #[test]
    fn stacked_scripts_keep_their_gap() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("x_i^2");
        let sup = l.glyphs.iter().find(|g| g.y < 0.0).expect("sup");
        let sub = l.glyphs.iter().find(|g| g.y > 0.0).expect("sub");
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let sup_bottom = sup.y - f.bounds(sup.glyph).y_min * script_scale;
        let sub_top = sub.y - f.bounds(sub.glyph).y_max * script_scale;
        let gap = sub_top - sup_bottom;
        assert!(
            gap + EPS >= c.sub_superscript_gap_min * scale(),
            "gap {} under minimum",
            gap
        );
    }

    #[test]
    fn italic_correction_pushes_the_superscript_right() {
        let sup_x = lay("f^2").glyphs[1].x;
        let sub_x = lay("f_2").glyphs[1].x;
        assert!(sup_x > sub_x + 0.1, "sup {} vs sub {}", sup_x, sub_x);
    }

    #[test]
    fn space_after_script_separates_the_next_atom() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("x^2y");
        let y = l
            .glyphs
            .iter()
            .find(|g| g.glyph == glyph_of('\u{1D466}'))
            .unwrap();
        let sup = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let sup_end = sup.x + font().advance(sup.glyph) * script_scale;
        assert!(y.x + EPS >= sup_end + c.space_after_script * scale());
    }

    #[test]
    fn literal_fallback_reserves_its_measured_box() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\foobar x");
        assert_eq!(l.literals.len(), 1);
        let lit = &l.literals[0];
        assert_eq!(lit.text, "\\foobar");
        assert_eq!(lit.x, 0.0);
        assert!((lit.width - f.measure_literal("\\foobar", SIZE)).abs() < EPS);
        assert_eq!(lit.source, 0..7);
        let x = l.glyphs.first().expect("x renders");
        assert!(x.x >= lit.width);
    }

    #[test]
    fn groups_flatten_onto_the_shared_baseline() {
        let l = lay("{ab}c");
        assert_eq!(l.glyphs.len(), 3);
        assert!(l.glyphs.windows(2).all(|w| w[0].x < w[1].x));
        assert!(l.glyphs.iter().all(|g| g.y == 0.0));
    }

    #[test]
    fn source_stamps_point_into_the_tex() {
        let l = lay("x^2");
        assert_eq!(l.glyphs[0].source, 0..1);
        assert_eq!(l.glyphs[1].source, 2..3);
    }

    #[test]
    fn empty_and_hostile_inputs_yield_finite_geometry() {
        for tex in ["", "^", "{", "}}", "x^", "\\", "%"] {
            let l = lay(tex);
            assert!(l.width.is_finite() && l.ascent.is_finite() && l.descent.is_finite());
        }
        assert_eq!(lay("").width, 0.0);
    }
}

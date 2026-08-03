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

/// A glyph stretched along the vertical axis: one ladder pick or the parts
/// of an assembly, in stack coordinates with the ink bottom at zero.
struct Stretched {
    glyphs: Vec<(crate::font::GlyphId, f32)>,
    width: f32,
    height: f32,
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
                if atom.class == AtomClass::Op {
                    return self.operator_box(atom, glyph, style);
                }
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
            Field::Fraction {
                numerator,
                denominator,
                bar,
            } => self.fraction_box(numerator, denominator, *bar, style, cramped),
            Field::Radical { radicand, degree } => self.radical_box(atom, radicand, degree, style),
            Field::LeftRight { open, close, body } => {
                self.left_right_box(atom, *open, *close, body, style, cramped)
            }
            Field::Empty => LaidBox {
                geom: MathLayout::default(),
                italic: 0.0,
            },
        }
    }

    /// Pushes a glyph at a position and widens the box's vertical extents
    /// by its ink, the bookkeeping `merge` does for whole boxes.
    fn push_glyph(
        &self,
        geom: &mut MathLayout,
        glyph: crate::font::GlyphId,
        x: f32,
        y: f32,
        style: MathStyle,
        source: &Range<usize>,
    ) {
        let u = self.unit(style);
        let b = self.font.bounds(glyph);
        geom.glyphs.push(PositionedGlyph {
            glyph,
            x,
            y,
            size: self.em(style),
            source: source.clone(),
        });
        geom.ascent = geom.ascent.max(-(y - b.y_max * u));
        geom.descent = geom.descent.max(y - b.y_min * u);
    }

    /// A glyph stretched vertically to cover `target`: the base, then the
    /// font's size ladder, then a glyph assembly from extenders. Offsets
    /// are stack coordinates: ink bottom at zero, growing upward.
    fn stretch_vertical(&self, ch: char, target: f32, style: MathStyle) -> Option<Stretched> {
        let u = self.unit(style);
        let base = self.font.glyph(ch)?;
        let ink = |g: crate::font::GlyphId| {
            let b = self.font.bounds(g);
            ((b.y_max - b.y_min) * u, b.y_min * u)
        };
        let (mut h, mut y_min) = ink(base);
        let mut chosen = base;
        if h < target {
            for v in self.font.vertical_variants(base) {
                let (vh, vy) = ink(v.glyph);
                chosen = v.glyph;
                h = vh;
                y_min = vy;
                if vh >= target {
                    break;
                }
            }
        }
        let assembly = self.font.vertical_assembly(base);
        if h >= target || assembly.is_none() {
            return Some(Stretched {
                glyphs: vec![(chosen, y_min)],
                width: self.font.advance(chosen) * u,
                height: h,
            });
        }
        let asm = assembly.expect("checked");
        let overlap = self.font.constants().min_connector_overlap * u;
        for reps in 1..=64usize {
            let mut parts: Vec<&crate::font::AssemblyPart> = Vec::new();
            for p in &asm.parts {
                if p.is_extender {
                    for _ in 0..reps {
                        parts.push(p);
                    }
                } else {
                    parts.push(p);
                }
            }
            let total: f32 = parts.iter().map(|p| p.full_advance * u).sum::<f32>()
                - overlap * parts.len().saturating_sub(1) as f32;
            if total >= target || reps == 64 {
                let mut glyphs = Vec::new();
                let mut width = 0.0f32;
                let mut cursor = 0.0;
                for p in &parts {
                    let b = self.font.bounds(p.glyph);
                    glyphs.push((p.glyph, -cursor + b.y_min * u));
                    width = width.max(self.font.advance(p.glyph) * u);
                    cursor += p.full_advance * u - overlap;
                }
                return Some(Stretched {
                    glyphs,
                    width,
                    height: total,
                });
            }
        }
        None
    }

    /// Appendix G's radical: the argument cramped in its own style, the
    /// surd stretched to cover it plus the gap and the bar, the optional
    /// degree raised before the surd at scriptscript size.
    fn radical_box(
        &self,
        atom: &Atom,
        radicand: &MathList,
        degree: &Option<MathList>,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let inner = self.hlist(radicand, style, true);
        let gap = if style == MathStyle::Display {
            c.radical_display_style_vertical_gap * u
        } else {
            c.radical_vertical_gap * u
        };
        let t = c.radical_rule_thickness * u;
        let target = inner.ascent + inner.descent + gap + t;
        let Some(surd) = self.stretch_vertical('\u{221A}', target, style) else {
            return self.literal_box("\\sqrt", atom, style);
        };
        // Rule 11: the bar joins the surd's top; a surd taller than the
        // target splits its excess, half widening the clearance above the
        // radicand, the rest hanging below.
        let clearance = gap + (surd.height - target).max(0.0) / 2.0;
        let bar_y = -(inner.ascent + clearance + t);
        let surd_bottom = bar_y + surd.height;

        let mut geom = MathLayout::default();
        let mut x = 0.0;
        if let Some(deg_list) = degree {
            let deg = self.hlist(deg_list, MathStyle::ScriptScript, false);
            // Two visual floors over the font's constants, the corner
            // every renderer tunes by hand: the raise floors at TeX's
            // classic 60 percent so the degree clears left-tick surd
            // designs, and a quarter of the kern's tuck stays untucked so
            // the degree keeps air from the diagonal.
            let percent = c.radical_degree_bottom_raise_percent.max(60.0);
            let raise = percent / 100.0 * surd.height;
            let deg_w = deg.width;
            let deg_y = surd_bottom - raise;
            x += c.radical_kern_before_degree * u;
            merge(&mut geom, deg, x, deg_y);
            x += deg_w + 0.75 * c.radical_kern_after_degree * u;
            x = x.max(0.0);
        }
        for (g, y_off) in &surd.glyphs {
            self.push_glyph(
                &mut geom,
                *g,
                x,
                y_off + surd_bottom,
                style,
                &atom.nucleus_span,
            );
        }
        let inner_x = x + surd.width;
        // The bar reaches back over the surd's flat terminal, so a
        // subpixel phase difference between glyph and rect rasterization
        // lands inside shared ink instead of showing as a notch.
        let overlap = (1.25 * t).min(0.15 * surd.width);
        geom.rules.push(Rule {
            x: inner_x - overlap,
            y: bar_y,
            width: inner.width + overlap,
            height: t,
        });
        let inner_w = inner.width;
        merge(&mut geom, inner, inner_x, 0.0);
        geom.ascent = geom.ascent.max(-bar_y + c.radical_extra_ascender * u);
        geom.width = inner_x + inner_w;
        LaidBox { geom, italic: 0.0 }
    }

    /// Appendix G's operator symbol: the display style swaps in a variant
    /// tall enough for `DisplayOperatorMinHeight`, and every style centers
    /// the glyph on the math axis.
    fn operator_box(&self, atom: &Atom, base: crate::font::GlyphId, style: MathStyle) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let mut glyph = base;
        if style == MathStyle::Display {
            let min_h = c.display_operator_min_height * u;
            let ink = |g: crate::font::GlyphId| {
                let b = self.font.bounds(g);
                (b.y_max - b.y_min) * u
            };
            if ink(glyph) < min_h {
                for v in self.font.vertical_variants(base) {
                    glyph = v.glyph;
                    if ink(v.glyph) >= min_h {
                        break;
                    }
                }
            }
        }
        let b = self.font.bounds(glyph);
        let y = (b.y_max + b.y_min) / 2.0 * u - c.axis_height * u;
        let mut geom = MathLayout::default();
        self.push_glyph(&mut geom, glyph, 0.0, y, style, &atom.nucleus_span);
        geom.width = self.font.advance(glyph) * u;
        LaidBox {
            geom,
            italic: self.font.italic_correction(glyph) * u,
        }
    }

    /// Whether an operator's `Limits::Default` resolves to stacked limits
    /// in display style: TeX's convention, everything except integrals.
    fn op_defaults_to_limits(nucleus: &Field) -> bool {
        !matches!(nucleus, Field::Symbol(c) if matches!(*c, '\u{222B}'..='\u{2233}'))
    }

    /// Rule 13a: limits centered above and below the operator, the gap and
    /// baseline-rise constants both honored, italic correction splitting
    /// the horizontal centers apart.
    fn limits_box(
        &self,
        atom: &Atom,
        nucleus: LaidBox,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let sub_style = Self::script_style(style);
        let op = nucleus.geom;
        let ic = nucleus.italic;
        let upper = atom.sup.as_ref().map(|l| self.hlist(l, sub_style, cramped));
        let lower = atom.sub.as_ref().map(|l| self.hlist(l, sub_style, true));
        let width = op
            .width
            .max(upper.as_ref().map(|b| b.width).unwrap_or(0.0))
            .max(lower.as_ref().map(|b| b.width).unwrap_or(0.0));
        let mut geom = MathLayout::default();
        let op_ascent = op.ascent;
        let op_descent = op.descent;
        let op_w = op.width;
        merge(&mut geom, op, (width - op_w) / 2.0, 0.0);
        if let Some(up) = upper {
            let rise =
                (c.upper_limit_baseline_rise_min * u).max(c.upper_limit_gap_min * u + up.descent);
            let x = (width - up.width) / 2.0 + ic / 2.0;
            merge(&mut geom, up, x, -(op_ascent + rise));
        }
        if let Some(low) = lower {
            let drop =
                (c.lower_limit_baseline_drop_min * u).max(c.lower_limit_gap_min * u + low.ascent);
            let x = (width - low.width) / 2.0 - ic / 2.0;
            merge(&mut geom, low, x, op_descent + drop);
        }
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// Rule 19: the body lays out first, then each delimiter stretches to
    /// cover its extent around the axis, honoring the delimited-formula
    /// minimum, and centers on the axis.
    fn left_right_box(
        &self,
        atom: &Atom,
        open: Option<char>,
        close: Option<char>,
        body_list: &MathList,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let body = self.hlist(body_list, style, cramped);
        let axis = c.axis_height * u;
        let half = (body.ascent - axis).max(body.descent + axis).max(0.0);
        let target = (2.0 * half).max(c.delimited_sub_formula_min_height * u);
        let mut geom = MathLayout::default();
        let mut x = 0.0;
        if let Some(ch) = open {
            x += self.place_delimiter(&mut geom, ch, target, axis, x, style, &atom.nucleus_span);
        }
        let body_w = body.width;
        merge(&mut geom, body, x, 0.0);
        x += body_w;
        if let Some(ch) = close {
            x += self.place_delimiter(&mut geom, ch, target, axis, x, style, &atom.nucleus_span);
        }
        geom.width = x;
        LaidBox { geom, italic: 0.0 }
    }

    /// One stretched delimiter centered on the axis at `x`; answers its
    /// advance. A character the font cannot draw reserves a literal box.
    #[allow(clippy::too_many_arguments)]
    fn place_delimiter(
        &self,
        geom: &mut MathLayout,
        ch: char,
        target: f32,
        axis: f32,
        x: f32,
        style: MathStyle,
        span: &Range<usize>,
    ) -> f32 {
        let Some(stretched) = self.stretch_vertical(ch, target, style) else {
            let em = self.em(style);
            let text = ch.to_string();
            let width = self.font.measure_literal(&text, em);
            geom.literals.push(Literal {
                text,
                x,
                y: 0.0,
                size: em,
                width,
                source: span.clone(),
            });
            geom.ascent = geom.ascent.max(em * 0.8);
            geom.descent = geom.descent.max(em * 0.2);
            return width;
        };
        let bottom = -axis + stretched.height / 2.0;
        for (g, y_off) in &stretched.glyphs {
            self.push_glyph(geom, *g, x, y_off + bottom, style, span);
        }
        stretched.width
    }

    /// The style a fraction's parts take, one size down from the whole.
    fn fraction_sub_style(style: MathStyle) -> MathStyle {
        match style {
            MathStyle::Display => MathStyle::Text,
            MathStyle::Text => MathStyle::Script,
            MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
        }
    }

    /// Appendix G's fraction: parts one style down, denominator cramped,
    /// the bar centered on the axis with the minimum gaps enforced. The
    /// barless form is TeX's stack, `\binom`'s interior, on the stack
    /// constants.
    fn fraction_box(
        &self,
        num_list: &MathList,
        den_list: &MathList,
        bar: bool,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let display = style == MathStyle::Display;
        let sub_style = Self::fraction_sub_style(style);
        let num = self.hlist(num_list, sub_style, cramped);
        let den = self.hlist(den_list, sub_style, true);
        let width = num.width.max(den.width);
        let mut geom = MathLayout::default();

        let (mut up, mut down) = if display {
            (
                c.fraction_numerator_display_style_shift_up * u,
                c.fraction_denominator_display_style_shift_down * u,
            )
        } else {
            (
                c.fraction_numerator_shift_up * u,
                c.fraction_denominator_shift_down * u,
            )
        };
        if bar {
            let axis = c.axis_height * u;
            let t = c.fraction_rule_thickness * u;
            let (gap_num, gap_den) = if display {
                (
                    c.fraction_num_display_style_gap_min * u,
                    c.fraction_denom_display_style_gap_min * u,
                )
            } else {
                (
                    c.fraction_numerator_gap_min * u,
                    c.fraction_denominator_gap_min * u,
                )
            };
            up = up.max(axis + t / 2.0 + gap_num + num.descent);
            down = down.max(den.ascent - axis + t / 2.0 + gap_den);
            geom.rules.push(Rule {
                x: 0.0,
                y: -(axis + t / 2.0),
                width,
                height: t,
            });
        } else {
            let (stack_up, stack_down, gap_min) = if display {
                (
                    c.stack_top_display_style_shift_up * u,
                    c.stack_bottom_display_style_shift_down * u,
                    c.stack_display_style_gap_min * u,
                )
            } else {
                (
                    c.stack_top_shift_up * u,
                    c.stack_bottom_shift_down * u,
                    c.stack_gap_min * u,
                )
            };
            up = stack_up;
            down = stack_down;
            let gap = (up - num.descent) - (den.ascent - down);
            if gap < gap_min {
                let half = (gap_min - gap) / 2.0;
                up += half;
                down += half;
            }
        }

        let (num_w, den_w) = (num.width, den.width);
        merge(&mut geom, num, (width - num_w) / 2.0, -up);
        merge(&mut geom, den, (width - den_w) / 2.0, down);
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
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
        let stacked = atom.class == AtomClass::Op
            && match atom.limits {
                crate::mlist::Limits::Limits => true,
                crate::mlist::Limits::NoLimits => false,
                crate::mlist::Limits::Default => {
                    style == MathStyle::Display && Self::op_defaults_to_limits(&atom.nucleus)
                }
            };
        if stacked {
            return self.limits_box(atom, nucleus, style, cramped);
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

        // Shifts are positive: sup up, sub down. Rule 18a: a character
        // nucleus takes the style constants alone; a box nucleus carries
        // its scripts with its own extent less the baseline drops.
        let is_char = matches!(atom.nucleus, Field::Symbol(_));
        let mut sup_shift = 0.0f32;
        if let Some(s) = &sup {
            let plain = if cramped {
                c.superscript_shift_up_cramped
            } else {
                c.superscript_shift_up
            } * u;
            sup_shift = plain.max(s.descent + c.superscript_bottom_min * u);
            if !is_char {
                sup_shift = sup_shift.max(geom.ascent - c.superscript_baseline_drop_max * u);
            }
        }
        let mut sub_shift = 0.0f32;
        if let Some(s) = &sub {
            sub_shift = (c.subscript_shift_down * u).max(s.ascent - c.subscript_top_max * u);
            if !is_char {
                sub_shift = sub_shift.max(geom.descent + c.subscript_baseline_drop_min * u);
            }
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
    fn fraction_bar_centers_on_the_axis_at_rule_thickness() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("\\frac{1}{2}");
        assert_eq!(l.rules.len(), 1);
        let bar = &l.rules[0];
        let t = c.fraction_rule_thickness * scale();
        let axis = c.axis_height * scale();
        assert!((bar.height - t).abs() < EPS);
        assert!(
            (bar.y - (-(axis + t / 2.0))).abs() < EPS,
            "bar top {} vs axis {}",
            bar.y,
            axis
        );
        assert!((bar.width - l.width).abs() < 1.0, "bar spans the fraction");
        // Text style sets numerator and denominator at script size.
        let sub_size = SIZE * c.script_percent_scale_down / 100.0;
        assert!(l.glyphs.iter().all(|g| (g.size - sub_size).abs() < EPS));
        let one = &l.glyphs[0];
        assert!(one.y < bar.y, "numerator above the bar");
        assert!(l.glyphs[1].y > 0.0, "denominator below the baseline");
    }

    #[test]
    fn display_style_raises_the_numerator_higher_at_full_size() {
        use crate::MathFont as _;
        let c = font().constants();
        let text = lay("\\frac{1}{2}");
        let disp = layout("\\frac{1}{2}", MathStyle::Display, SIZE, &font());
        assert!(
            (disp.glyphs[0].size - SIZE).abs() < EPS,
            "display numerator at text size"
        );
        assert!(
            disp.glyphs[0].y < text.glyphs[0].y - 1.0,
            "display shift-up beats text: {} vs {}",
            disp.glyphs[0].y,
            text.glyphs[0].y
        );
        assert!(disp.ascent > text.ascent);
        let _ = c;
    }

    #[test]
    fn deep_numerator_honors_the_bar_gap() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\frac{g}{2}");
        let axis = c.axis_height * scale();
        let t = c.fraction_rule_thickness * scale();
        let gap = c.fraction_numerator_gap_min * scale();
        let g = &l.glyphs[0];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let g_bottom = g.y - f.bounds(g.glyph).y_min * script_scale;
        assert!(
            g_bottom <= -(axis + t / 2.0 + gap) + EPS,
            "numerator ink clears the bar gap, bottom {}",
            g_bottom
        );
    }

    #[test]
    fn fractions_space_as_inner_atoms() {
        let l = lay("a\\frac{1}{2}");
        let thin = 3.0 / 18.0 * SIZE;
        let a_end = adv('\u{1D44E}');
        let frac_first = l
            .glyphs
            .iter()
            .skip(1)
            .map(|g| g.x)
            .fold(f32::MAX, f32::min);
        assert!(
            frac_first + EPS >= a_end + thin,
            "Ord-Inner thin space, start {} vs {}",
            frac_first,
            a_end + thin
        );
        let _ = font();
    }

    #[test]
    fn radical_draws_surd_and_overbar_over_the_argument() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\sqrt{x}");
        assert_eq!(l.rules.len(), 1, "the overbar");
        let bar = &l.rules[0];
        let t = c.radical_rule_thickness * scale();
        assert!((bar.height - t).abs() < EPS);
        assert!(bar.y < -SIZE * 0.3, "bar above the radicand");
        let x_glyph = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D465}').unwrap())
            .expect("radicand renders");
        let surd = l
            .glyphs
            .iter()
            .find(|g| g.glyph != x_glyph.glyph)
            .expect("surd renders");
        assert!(surd.x < x_glyph.x, "surd before the radicand");
        assert!(bar.x + bar.width > x_glyph.x, "bar covers the radicand");
        assert!(l.ascent >= -bar.y + c.radical_extra_ascender * scale() - EPS);
        // Rule 11: the bar joins the surd's ink top exactly; a surd taller
        // than needed hangs below, never pokes above the bar.
        let surd_top = surd.y - f.bounds(surd.glyph).y_max * scale();
        assert!(
            (surd_top - bar.y).abs() < EPS,
            "bar top {} meets surd top {}",
            bar.y,
            surd_top
        );
    }

    #[test]
    fn taller_arguments_take_taller_surds() {
        use crate::MathFont as _;
        let f = font();
        let x_id = f.glyph('\u{1D465}').unwrap();
        let small = lay("\\sqrt{x}");
        let tall = layout("\\sqrt{\\frac{1}{2}}", MathStyle::Display, SIZE, &font());
        let small_surd = small.glyphs.iter().find(|g| g.glyph != x_id).unwrap();
        let tall_surds: Vec<_> = tall
            .glyphs
            .iter()
            .filter(|g| {
                let ink = f.bounds(g.glyph);
                (ink.y_max - ink.y_min) * scale() > SIZE * 0.8
            })
            .collect();
        assert!(
            tall_surds.iter().any(|g| g.glyph != small_surd.glyph) || tall_surds.len() > 1,
            "a taller variant or an assembly serves the tall argument"
        );
        assert!(tall.ascent + tall.descent > small.ascent + small.descent + 4.0);
    }

    #[test]
    fn degree_raises_small_before_the_surd() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\sqrt[3]{x}");
        let three = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('3').unwrap())
            .expect("degree renders");
        let ss = SIZE * c.script_script_percent_scale_down / 100.0;
        assert!((three.size - ss).abs() < EPS, "degree at scriptscript size");
        assert!(three.y < 0.0, "degree raised above the baseline");
        let x_glyph = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D465}').unwrap())
            .unwrap();
        assert!(three.x < x_glyph.x, "degree before the radicand");
        // The raise floors at the classic 60 percent even when the font
        // asks lower, clearing left-tick surd designs.
        let surd = l
            .glyphs
            .iter()
            .find(|g| {
                let b = f.bounds(g.glyph);
                (b.y_max - b.y_min) * scale() > SIZE * 0.8
            })
            .expect("surd");
        let sb = f.bounds(surd.glyph);
        let ink_h = (sb.y_max - sb.y_min) * scale();
        let surd_bottom = surd.y - sb.y_min * scale();
        let raise = c.radical_degree_bottom_raise_percent.max(60.0) / 100.0;
        assert!(
            (three.y - (surd_bottom - raise * ink_h)).abs() < 0.5,
            "degree bottom at the floored raise, y {} vs {}",
            three.y,
            surd_bottom - raise * ink_h
        );
        // A quarter of the tuck stays untucked so the degree keeps air
        // from the diagonal.
        let deg_w =
            f.advance(f.glyph('3').unwrap()) * scale() * c.script_script_percent_scale_down / 100.0;
        let expected_surd_x = (c.radical_kern_before_degree * scale()
            + deg_w
            + 0.75 * c.radical_kern_after_degree * scale())
        .max(0.0);
        assert!(
            (surd.x - expected_surd_x).abs() < 0.5,
            "surd tucks by three quarters of the kern, x {} vs {}",
            surd.x,
            expected_surd_x
        );
    }

    #[test]
    fn radical_bar_overlaps_the_surd_terminal() {
        let l = lay("\\sqrt{x}");
        let bar = &l.rules[0];
        let surd_end = l.glyphs[0].x + {
            use crate::MathFont as _;
            font().advance(l.glyphs[0].glyph) * scale()
        };
        assert!(
            bar.x < surd_end - 1.0,
            "the bar starts inside the surd's terminal, {} vs {}",
            bar.x,
            surd_end
        );
        assert!(
            bar.x + bar.width > surd_end,
            "and still covers the radicand"
        );
    }

    #[test]
    fn display_style_enlarges_operators_onto_the_axis() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let text = lay("\\sum");
        let disp = layout("\\sum", MathStyle::Display, SIZE, &font());
        assert_ne!(
            text.glyphs[0].glyph, disp.glyphs[0].glyph,
            "display takes a larger variant"
        );
        let g = &disp.glyphs[0];
        let b = f.bounds(g.glyph);
        let ink_h = (b.y_max - b.y_min) * scale();
        let tb = f.bounds(text.glyphs[0].glyph);
        let text_h = (tb.y_max - tb.y_min) * scale();
        // The ladder answers the largest it has; STIX tops out below the
        // constant for the summation sign, so taller-than-text is the
        // honest contract.
        assert!(
            ink_h > text_h * 1.2,
            "display ink {} vs text {}",
            ink_h,
            text_h
        );
        let center = g.y - (b.y_max + b.y_min) / 2.0 * scale();
        assert!(
            (center - (-c.axis_height * scale())).abs() < 0.5,
            "centered on the axis, center {}",
            center
        );
    }

    #[test]
    fn display_limits_stack_above_and_below() {
        use crate::MathFont as _;
        let f = font();
        let disp = layout("\\sum_{i}^{n}", MathStyle::Display, SIZE, &font());
        let op = disp
            .glyphs
            .iter()
            .max_by(|a, b| a.size.partial_cmp(&b.size).unwrap())
            .expect("operator");
        let ob = f.bounds(op.glyph);
        let op_top = op.y - ob.y_max * scale();
        let op_bottom = op.y - ob.y_min * scale();
        let op_center = op.x + f.advance(op.glyph) * scale() / 2.0;
        let upper = disp
            .glyphs
            .iter()
            .find(|g| g.y < op_top)
            .expect("upper limit");
        let lower = disp
            .glyphs
            .iter()
            .find(|g| g.y > op_bottom)
            .expect("lower limit");
        for lim in [upper, lower] {
            let lw = f.advance(lim.glyph) * scale() * lim.size / SIZE;
            let center = lim.x + lw / 2.0;
            assert!(
                (center - op_center).abs() < SIZE * 0.4,
                "limit roughly centered, {} vs {}",
                center,
                op_center
            );
        }
    }

    #[test]
    fn text_style_and_nolimits_keep_side_scripts() {
        use crate::MathFont as _;
        let f = font();
        let side = |l: &MathLayout| {
            let op = &l.glyphs[0];
            let end = op.x + f.advance(op.glyph) * scale();
            l.glyphs.iter().skip(1).all(|g| g.x >= end - 1.0)
        };
        assert!(side(&lay("\\sum_i^n")), "text style scripts sit beside");
        assert!(
            side(&layout(
                "\\sum\\nolimits_i^n",
                MathStyle::Display,
                SIZE,
                &font()
            )),
            "nolimits forces the side form in display"
        );
        assert!(
            side(&layout("\\int_0^1", MathStyle::Display, SIZE, &font())),
            "integrals default to side scripts"
        );
        let stacked = layout("\\int\\limits_0^1", MathStyle::Display, SIZE, &font());
        assert!(
            !side(&stacked),
            "explicit limits override the integral default"
        );
    }

    #[test]
    fn left_right_stretches_to_cover_the_body() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let base_paren = f.glyph('(').unwrap();
        let l = layout(
            "\\left(\\frac{1}{\\frac{2}{3}}\\right)",
            MathStyle::Display,
            SIZE,
            &font(),
        );
        let open: Vec<_> = l
            .glyphs
            .iter()
            .filter(|g| g.x < l.glyphs.iter().map(|h| h.x).fold(f32::MAX, f32::min) + 0.5)
            .collect();
        assert!(
            open.iter().all(|g| g.glyph != base_paren),
            "a ladder variant or assembly serves, never the base paren"
        );
        let ink_top = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_max * scale())
            .fold(f32::MAX, f32::min);
        let ink_bottom = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_min * scale())
            .fold(0.0, f32::max);
        let axis = c.axis_height * scale();
        let need = (l.ascent - axis).max(l.descent + axis);
        assert!(
            ink_bottom - ink_top + 1.0 >= need,
            "delimiter ink {} covers the body's {}",
            ink_bottom - ink_top,
            need
        );
        let center = (ink_top + ink_bottom) / 2.0;
        assert!(
            (center - (-axis)).abs() < 1.5,
            "delimiter centers on the axis, center {}",
            center
        );
    }

    #[test]
    fn binom_stacks_inside_parens_without_a_bar() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\binom{n}{k}");
        assert!(l.rules.is_empty(), "no fraction bar");
        let n = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D45B}').unwrap())
            .expect("n renders");
        let k = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D458}').unwrap())
            .expect("k renders");
        assert!(n.y < 0.0 && k.y > 0.0, "stacked around the baseline");
        let leftmost = l.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(n.x > leftmost, "a delimiter sits before the stack");
        assert!(l.width > f.advance(f.glyph('\u{1D45B}').unwrap()) * scale() * 2.0);
    }

    #[test]
    fn scripts_ride_the_delimited_group() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\left(x\\right)^2");
        let two = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("the script renders");
        let rightmost_other = l
            .glyphs
            .iter()
            .filter(|g| g.glyph != two.glyph)
            .map(|g| g.x)
            .fold(0.0, f32::max);
        assert!(two.x > rightmost_other, "the script follows the closer");
        assert!(two.y < 0.0, "raised as a superscript");
    }

    #[test]
    fn scripts_on_boxes_rise_and_drop_with_the_box() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let tall = "\\left(\\frac{1}{1+\\frac{1}{x}}\\right)";
        let plain = lay(tall);
        let scripted = lay(&format!("{tall}^2"));
        let two = scripted
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("superscript renders");
        // Rule 18a: on a box nucleus the shift is the box height less the
        // baseline drop, which dwarfs the character constant here.
        let expected = plain.ascent - c.superscript_baseline_drop_max * scale();
        assert!(
            (two.y - (-expected)).abs() < 0.6,
            "sup baseline {} vs box rule {}",
            two.y,
            -expected
        );
        let subbed = lay(&format!("{tall}_2"));
        let two = subbed
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("subscript renders");
        let expected = plain.descent + c.subscript_baseline_drop_min * scale();
        assert!(
            (two.y - expected).abs() < 0.6,
            "sub baseline {} vs box rule {}",
            two.y,
            expected
        );
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
